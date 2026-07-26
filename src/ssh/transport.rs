//! SSH transport: russh over `gh cs ssh --stdio` subprocess.
//!
//! Implements pure-Rust SSH connectivity to GitHub Codespaces by:
//! 1. Spawning `gh cs ssh -c <codespace_name> --stdio` as a tokio subprocess
//!    with stdin/stdout piped (stderr inherited so the operator can see gh's
//!    diagnostics).
//! 2. Wrapping the subprocess's stdin/stdout into a single type that implements
//!    `tokio::io::AsyncRead + AsyncWrite` — that's the contract russh's
//!    `connect_stream` requires.
//! 3. Generating (or loading) an Ed25519 keypair at
//!    `~/.cache/codespacectl/id_codespace` (PKCS#8 PEM on disk, 0600 perms on
//!    Unix) for use as the SSH client identity.
//! 4. Driving the SSH handshake and public-key auth (`codespace` user) via
//!    `russh::client::connect_stream` + `authenticate_publickey`.
//! 5. Capturing the server host key fingerprint through the
//!    `check_server_key` callback so the caller can do TOFU against the
//!    per-codespace state file.
//!
//! All SSH protocol work happens in-process — no system `ssh` binary, no
//! `ssh-keygen`, no `ssh-agent` required.

use crate::{CodespaceError, Result};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::ssh::host_keys;

/// Default SSH connect/auth timeout in seconds.
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 30;

/// SSH username used by GitHub Codespaces.
const CODESPACE_SSH_USER: &str = "codespace";

/// Path to the SSH private key used for codespace auth.
///
/// Deterministic: same path every time, so the key persists across sessions.
/// Located under `~/.cache/codespacectl/id_codespace` (or platform equivalent
/// via `dirs::cache_dir()`).
pub fn ssh_key_path() -> PathBuf {
    super_key_dir().join("id_codespace")
}

/// Directory holding codespacectl's cache artifacts (SSH key, state file).
fn super_key_dir() -> PathBuf {
    let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp/.cache"));
    cache.join("codespacectl")
}

/// Result of an SSH exec command.
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Error from SSH operations.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("failed to spawn gh subprocess: {0}")]
    SpawnFailed(String),
    #[error("gh subprocess exited: {0}")]
    SubprocessExited(String),
    #[error("SSH handshake failed: {0}")]
    HandshakeFailed(String),
    #[error("SSH auth failed: {0}")]
    AuthFailed(String),
    #[error("command execution failed: {0}")]
    ExecFailed(String),
    #[error("transport closed unexpectedly")]
    TransportClosed,
    #[error("russh error: {0}")]
    RusshError(String),
    #[error("russh-keys error: {0}")]
    RusshKeysError(String),
    #[error("key generation failed: {0}")]
    KeyGenFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> Self {
        SshError::RusshError(e.to_string())
    }
}

impl From<russh_keys::Error> for SshError {
    fn from(e: russh_keys::Error) -> Self {
        SshError::RusshKeysError(e.to_string())
    }
}

impl From<SshError> for CodespaceError {
    fn from(e: SshError) -> Self {
        match e {
            SshError::SpawnFailed(_) | SshError::SubprocessExited(_) => {
                CodespaceError::CodespaceUnreachable(e.to_string())
            }
            SshError::HandshakeFailed(_) | SshError::AuthFailed(_) => {
                CodespaceError::HostKeyMismatch {
                    expected: "unknown".into(),
                    actual: e.to_string(),
                }
            }
            _ => CodespaceError::Internal(format!("ssh error: {}", e)),
        }
    }
}

// ---------------------------------------------------------------------------
// SshTransport: a single AsyncRead + AsyncWrite view over the gh subprocess's
// stdin (writes) and stdout (reads).
// ---------------------------------------------------------------------------

/// Adapter that exposes a tokio subprocess's `ChildStdout` (reads) and
/// `ChildStdin` (writes) as a single bidirectional stream for russh.
///
/// Both underlying halves are `Unpin`, so `SshTransport` itself is `Unpin`.
/// We use safe pin projection via `Pin::get_mut` (which is sound for `Unpin`
/// types) and `Pin::new` on the projected fields.
pub struct SshTransport {
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
}

impl AsyncRead for SshTransport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // Safe: SshTransport is Unpin (both fields are Unpin), so we can freely
        // obtain a &mut Self from Pin<&mut Self>.
        let this = self.get_mut();
        Pin::new(&mut this.stdout).poll_read(cx, buf)
    }
}

impl AsyncWrite for SshTransport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        Pin::new(&mut this.stdin).poll_write(cx, buf)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.stdin).poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.stdin).poll_shutdown(cx)
    }
}

// ---------------------------------------------------------------------------
// ClientHandler: russh client-side Handler.
//
// We accept the server's host key at the russh layer (TOFU is enforced at a
// higher level by comparing fingerprints with the codespace state file). The
// fingerprint of the server key seen during `check_server_key` is captured
// into an Arc<Mutex<Option<String>>> so the caller can read it after connect.
// ---------------------------------------------------------------------------

/// russh client handler. Captures the host key fingerprint during the SSH
/// handshake so the caller can perform TOFU verification against the
/// per-codespace state.
pub struct ClientHandler {
    /// SHA-256 fingerprint (formatted as `SHA256:<base64>`) of the server's
    /// public key as seen during the handshake. `None` until `check_server_key`
    /// has fired.
    pub host_key_fingerprint: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for ClientHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientHandler")
            .field("host_key_fingerprint", &"<Arc<Mutex<Option<String>>>")
            .finish()
    }
}

impl ClientHandler {
    /// Construct a handler with a fresh (empty) fingerprint slot.
    pub fn new() -> Self {
        Self {
            host_key_fingerprint: Arc::new(Mutex::new(None)),
        }
    }

    /// Clone the inner fingerprint handle so the caller can poll for it after
    /// `connect_stream` returns.
    pub fn fingerprint_handle(&self) -> Arc<Mutex<Option<String>>> {
        Arc::clone(&self.host_key_fingerprint)
    }
}

impl Default for ClientHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        // Compute the standard SSH-format fingerprint (SHA256 base64 nopad).
        let fp = format!("SHA256:{}", server_public_key.fingerprint());
        *self.host_key_fingerprint.lock().await = Some(fp);
        // Always accept at the russh layer — TOFU is enforced at a higher
        // level where we can compare against the per-codespace state file.
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// CodespaceSsh: top-level handle owning both the subprocess and russh session.
// ---------------------------------------------------------------------------

/// Top-level SSH connection handle. Owns the gh subprocess and russh session.
pub struct CodespaceSsh {
    pub child: Child,
    pub session: Option<russh::client::Handle<ClientHandler>>,
    pub codespace_name: String,
    /// Snapshot of the host key fingerprint captured during connect.
    /// `None` if `check_server_key` hasn't fired (e.g. handshake failed early).
    pub host_key_fingerprint: Option<String>,
}

impl CodespaceSsh {
    /// Spawn `gh cs ssh -c <name> --stdio` and establish an SSH session.
    ///
    /// `gh_bin` is the path to the gh CLI binary (typically vendored at
    /// `tools/bin/gh`). `timeout` bounds the whole connect + auth attempt.
    pub async fn connect(
        codespace_name: &str,
        gh_bin: &str,
        timeout: Duration,
    ) -> Result<Self> {
        let connect_timeout = if timeout.is_zero() {
            Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS)
        } else {
            timeout
        };

        // 1. Spawn `gh cs ssh -c <codespace_name> --stdio`.
        let mut cmd = Command::new(gh_bin);
        cmd.args(["cs", "ssh", "-c", codespace_name, "--stdio"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);

        // Pass GH_TOKEN through (prefer CODESPACECTL_TOKEN per project convention).
        let token_to_pass = std::env::var("CODESPACECTL_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("GH_TOKEN").ok().filter(|s| !s.is_empty()));
        if let Some(token) = token_to_pass {
            cmd.env("GH_TOKEN", token);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| SshError::SpawnFailed(format!("{}: {}", gh_bin, e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SshError::SpawnFailed("gh stdin not piped".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SshError::SpawnFailed("gh stdout not piped".into()))?;

        let transport = SshTransport { stdin, stdout };

        // 2. Ensure we have an Ed25519 keypair for auth.
        let key_pair = ensure_ssh_key()?;

        // 3. Build the russh client config.
        let config = russh::client::Config {
            inactivity_timeout: Some(connect_timeout),
            keepalive_interval: Some(Duration::from_secs(15)),
            ..<_>::default()
        };
        let config = Arc::new(config);

        // 4. Connect + auth within the timeout.
        let handler = ClientHandler::new();
        let fp_handle = handler.fingerprint_handle();

        let connect_fut = russh::client::connect_stream(config, transport, handler);

        let mut session = tokio::time::timeout(connect_timeout, connect_fut)
            .await
            .map_err(|_| {
                SshError::HandshakeFailed(format!(
                    "SSH connect timed out after {}s",
                    connect_timeout.as_secs()
                ))
            })?
            .map_err(|e| SshError::HandshakeFailed(e.to_string()))?;

        let auth_ok = tokio::time::timeout(
            connect_timeout,
            session.authenticate_publickey(CODESPACE_SSH_USER, Arc::new(key_pair)),
        )
        .await
        .map_err(|_| {
            SshError::AuthFailed(format!(
                "SSH auth timed out after {}s",
                connect_timeout.as_secs()
            ))
        })?
        .map_err(|e| SshError::AuthFailed(e.to_string()))?;

        if !auth_ok {
            return Err(SshError::AuthFailed(
                "server rejected publickey auth".into(),
            )
            .into());
        }

        // 5. Read the captured host key fingerprint (if check_server_key fired).
        let host_key_fingerprint = fp_handle.lock().await.clone();

        Ok(Self {
            child,
            session: Some(session),
            codespace_name: codespace_name.to_string(),
            host_key_fingerprint,
        })
    }

    /// Execute a shell command on the codespace.
    ///
    /// Returns stdout/stderr captured from the channel, plus the remote exit
    /// status. `timeout` bounds the entire exec + read loop.
    pub async fn exec(&mut self, command: &str, timeout: Duration) -> Result<ExecResult> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| SshError::TransportClosed)?;

        let mut channel = tokio::time::timeout(timeout, session.channel_open_session())
            .await
            .map_err(|_| {
                SshError::ExecFailed(format!(
                    "channel_open_session timed out after {}s",
                    timeout.as_secs()
                ))
            })?
            .map_err(|e| SshError::ExecFailed(format!("channel_open_session: {}", e)))?;

        // want_reply = false: russh's wait() loop surfaces ExitStatus regardless.
        channel
            .exec(false, command.as_bytes())
            .await
            .map_err(|e| SshError::ExecFailed(format!("exec: {}", e)))?;

        let mut stdout_buf: Vec<u8> = Vec::new();
        let mut stderr_buf: Vec<u8> = Vec::new();
        let mut exit_code: i32 = 0;
        let mut got_exit = false;

        let read_loop = async {
            while let Some(msg) = channel.wait().await {
                match msg {
                    russh::ChannelMsg::Data { ref data } => {
                        stdout_buf.extend_from_slice(data);
                    }
                    russh::ChannelMsg::ExtendedData { ref data, ext } => {
                        // ext == 1 is stderr per RFC 4254.
                        if ext == 1 {
                            stderr_buf.extend_from_slice(data);
                        } else {
                            // Unknown extended stream — fold into stdout for visibility.
                            stdout_buf.extend_from_slice(data);
                        }
                    }
                    russh::ChannelMsg::ExitStatus { exit_status } => {
                        exit_code = exit_status as i32;
                        got_exit = true;
                        // Don't break — there may be trailing data/EOF pending.
                    }
                    russh::ChannelMsg::Eof | russh::ChannelMsg::Close => {
                        // Channel is wrapping up; keep draining until wait() returns None.
                    }
                    _ => {}
                }
            }
        };

        tokio::time::timeout(timeout, read_loop)
            .await
            .map_err(|_| {
                SshError::ExecFailed(format!(
                    "exec read loop timed out after {}s",
                    timeout.as_secs()
                ))
            })?;

        if !got_exit {
            // Server closed the channel without sending ExitStatus — treat as failure.
            return Err(SshError::ExecFailed(
                "channel closed without exit status".into(),
            )
            .into());
        }

        Ok(ExecResult {
            stdout: String::from_utf8_lossy(&stdout_buf).into_owned(),
            stderr: String::from_utf8_lossy(&stderr_buf).into_owned(),
            exit_code,
        })
    }

    /// Close the SSH session and kill the subprocess.
    ///
    /// Best-effort: any errors from disconnect or kill are swallowed because
    /// we're tearing down anyway.
    pub async fn close(mut self) -> Result<()> {
        if let Some(handle) = self.session.take() {
            let _ = handle
                .disconnect(russh::Disconnect::ByApplication, "", "en")
                .await;
        }
        // Best-effort kill + wait. Ignore errors — we're tearing down.
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        Ok(())
    }

    /// Return the captured host key fingerprint from the most recent connect.
    pub fn host_key_fingerprint(&self) -> Option<&str> {
        self.host_key_fingerprint.as_deref()
    }
}

/// Top-level: spawn gh, connect SSH, return CodespaceSsh.
pub async fn connect_codespace(
    codespace_name: &str,
    gh_bin: &str,
    timeout_secs: u64,
) -> Result<CodespaceSsh> {
    let timeout = if timeout_secs == 0 {
        Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS)
    } else {
        Duration::from_secs(timeout_secs)
    };
    CodespaceSsh::connect(codespace_name, gh_bin, timeout).await
}

// ---------------------------------------------------------------------------
// SSH key management: generate (Ed25519, PKCS#8 PEM on disk) + load.
// ---------------------------------------------------------------------------

/// Ensure the codespacectl SSH keypair exists at `ssh_key_path()`. Generates
/// a fresh Ed25519 key on first use; loads from disk on subsequent calls.
pub fn ensure_ssh_key() -> Result<russh_keys::key::KeyPair> {
    let path = ssh_key_path();
    if path.exists() {
        load_ssh_key(&path)
    } else {
        generate_and_store_ssh_key(&path)
    }
}

/// Load a KeyPair from a PEM file on disk (OpenSSH or PKCS#8 format).
pub fn load_ssh_key(path: &Path) -> Result<russh_keys::key::KeyPair> {
    russh_keys::load_secret_key(path, None).map_err(SshError::from).map_err(Into::into)
}

/// Generate a fresh Ed25519 KeyPair, serialize as PKCS#8 PEM to `path`,
/// set 0600 perms on Unix, and return the in-memory KeyPair.
pub fn generate_and_store_ssh_key(path: &Path) -> Result<russh_keys::key::KeyPair> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            SshError::KeyGenFailed(format!(
                "failed to create key dir {}: {}",
                parent.display(),
                e
            ))
        })?;
    }

    let key_pair = russh_keys::key::KeyPair::generate_ed25519();

    // Serialize as PKCS#8 PEM. `russh_keys::encode_pkcs8_pem` writes a
    // `-----BEGIN PRIVATE KEY-----` block that decode_secret_key (used by
    // load_secret_key) can parse.
    let mut file = std::fs::File::create(path).map_err(|e| {
        SshError::KeyGenFailed(format!("failed to create key file {}: {}", path.display(), e))
    })?;

    russh_keys::encode_pkcs8_pem(&key_pair, &mut file).map_err(|e| {
        SshError::KeyGenFailed(format!("failed to encode key pair: {}", e))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            SshError::KeyGenFailed(format!("failed to set key file perms: {}", e))
        })?;
    }

    Ok(key_pair)
}

/// Compute the SSH-format fingerprint for a given host key, mirroring the
/// format used by OpenSSH (`SHA256:<base64-nopad>`).
///
/// Public helper so callers (state module, TOFU logic) can compute the same
/// string the ClientHandler captures during connect.
pub fn host_key_fingerprint(public_key: &russh_keys::key::PublicKey) -> String {
    format!("SHA256:{}", public_key.fingerprint())
}

// Re-export host_keys module members used by the TOFU path so callers can
// reach them through `crate::ssh::transport::*` if desired.
pub use host_keys::{HostKeyDecision, HostKeyError, HostKeyStore};

#[cfg(test)]
mod tests {
    use super::*;
    use russh_keys::PublicKeyBase64;
    use tokio::io::{AsyncRead as TokioAsyncRead, AsyncWrite as TokioAsyncWrite};

    // -----------------------------------------------------------------------
    // Type-level test: SshTransport implements AsyncRead + AsyncWrite.
    //
    // We use a free function with a trait bound instead of `static_assertions`
    // (not in deps). If SshTransport ever fails to implement these traits,
    // this function fails to compile.
    // -----------------------------------------------------------------------
    #[test]
    fn test_ssh_transport_pin_projection_compiles() {
        fn _assert_transport_traits<T: TokioAsyncRead + TokioAsyncWrite + Unpin>() {}
        _assert_transport_traits::<SshTransport>();
        // If we got here, SshTransport implements both AsyncRead and AsyncWrite
        // and is Unpin — pin projection via Pin::get_mut is sound.
    }

    // -----------------------------------------------------------------------
    // ssh_key_path is deterministic (same value every call within a process).
    // -----------------------------------------------------------------------
    #[test]
    fn test_ssh_key_path_is_deterministic() {
        let path1 = ssh_key_path();
        let path2 = ssh_key_path();
        assert_eq!(path1, path2, "ssh_key_path() must be deterministic");
        assert!(
            path1.ends_with("id_codespace"),
            "ssh_key_path() should end with id_codespace, got {}",
            path1.display()
        );
    }

    // -----------------------------------------------------------------------
    // Generating a key creates a file on disk with 0600 perms on Unix.
    // -----------------------------------------------------------------------
    #[test]
    fn test_ssh_key_generation_creates_file_with_correct_perms() {
        // Redirect cache dir into a tempdir so we don't clobber the real key.
        let tmp = tempfile::tempdir().expect("tempdir creation failed");
        let prev = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("XDG_CACHE_HOME", tmp.path());

        let path = ssh_key_path();
        let _key = generate_and_store_ssh_key(&path).expect("key generation failed");

        assert!(
            path.exists(),
            "key file should exist after generation: {}",
            path.display()
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&path).expect("metadata failed");
            let mode = metadata.permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "key file should have 0600 perms, got {:o}",
                mode
            );
        }

        // Restore env.
        if let Some(v) = prev {
            std::env::set_var("XDG_CACHE_HOME", v);
        } else {
            std::env::remove_var("XDG_CACHE_HOME");
        }
    }

    // -----------------------------------------------------------------------
    // Generated key loads back without error.
    // -----------------------------------------------------------------------
    #[test]
    fn test_ssh_key_load_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir creation failed");
        let prev = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("XDG_CACHE_HOME", tmp.path());

        let path = ssh_key_path();
        let original = generate_and_store_ssh_key(&path).expect("key generation failed");
        let loaded = load_ssh_key(&path).expect("key load failed");

        // Both keypairs should have the same public key bytes.
        let orig_pub = original.public_key_bytes();
        let loaded_pub = loaded.public_key_bytes();
        assert_eq!(
            orig_pub, loaded_pub,
            "loaded key public bytes must match generated key public bytes"
        );

        // Both should be Ed25519 (name() returns the algorithm string like "ssh-ed25519").
        assert_eq!(
            original.name(),
            "ssh-ed25519",
            "generated key should be Ed25519"
        );
        assert_eq!(loaded.name(), "ssh-ed25519", "loaded key should be Ed25519");

        if let Some(v) = prev {
            std::env::set_var("XDG_CACHE_HOME", v);
        } else {
            std::env::remove_var("XDG_CACHE_HOME");
        }
    }
}
