//! SSH transport: russh over `gh cs ssh --stdio` subprocess.
//!
//! TODO (Wave 4 subagent): implement.
//!
//! Approach:
//! 1. Spawn `gh cs ssh -c <codespace_name> --stdio` as a tokio::process::Command
//!    with stdin piped + stdout piped.
//! 2. Wait for the subprocess to be ready (gh writes a marker to stderr).
//! 3. Build a struct `SshTransport { stdin: ChildStdin, stdout: ChildStdout }`
//!    that implements `AsyncRead + AsyncWrite` by delegating to the respective
//!    pipe halves.
//! 4. Generate or load an Ed25519 key (preferred) or ECDSA key (fallback) at
//!    `~/.cache/codespacectl/ssh_key`.
//! 5. `russh::client::connect_stream(config, transport, handler)` — establishes
//!    SSH session over the transport.
//! 6. `handle.authenticate_publickey("codespace", key)` — auth.
//! 7. Open session channel, run command, read stdout/stderr, get exit code.
//!
//! Key points:
//! - NEVER use a fake ssh-keygen script in PATH — use direct russh API.
//! - TOFU host key: on first connect, persist the host key fingerprint to state;
//!   on subsequent connects, verify match (with rotation detection via codespace
//!   created_at timestamp).
//! - Ed25519 preferred; fall back to ECDSA if russh version doesn't support it.
//! - Always close subprocess + russh Transport in finally-style cleanup.

use crate::{CodespaceError, Result};
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::{Child, ChildStdin, ChildStdout};

/// Path to the SSH private key used for codespace auth.
/// Deterministic: same path every time, so the key persists across sessions.
pub fn ssh_key_path() -> PathBuf {
    super_key_dir().join("id_codespace")
}

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
    #[error("key generation failed: {0}")]
    KeyGenFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
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

/// Top-level SSH connection handle. Owns the subprocess and russh session.
pub struct CodespaceSsh {
    pub child: Child,
    pub session: Option<russh::client::Handle<ClientHandler>>,
    pub codespace_name: String,
}

/// Empty russh client handler. We don't need server-initiated operations.
#[derive(Debug, Clone)]
pub struct ClientHandler;

#[async_trait::async_trait]
impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        // We do TOFU at a higher level (comparing fingerprints with stored state).
        // At the russh layer, accept any key here — we'll capture the fingerprint
        // separately for persistence/verification.
        Ok(true)
    }
}

impl CodespaceSsh {
    /// Spawn `gh cs ssh -c <name> --stdio` and establish SSH session.
    pub async fn connect(
        codespace_name: &str,
        gh_bin: &str,
        timeout: Duration,
    ) -> Result<Self> {
        let _ = (codespace_name, gh_bin, timeout);
        // TODO (Wave 4 subagent): implement
        Err(CodespaceError::Internal(
            "SSH transport not yet implemented — Wave 4 subagent pending".into(),
        ))
    }

    /// Execute a shell command on the codespace.
    pub async fn exec(&mut self, command: &str, timeout: Duration) -> Result<ExecResult> {
        let _ = command;
        let _ = timeout;
        // TODO (Wave 4 subagent): implement
        Err(CodespaceError::Internal(
            "SSH exec not yet implemented — Wave 4 subagent pending".into(),
        ))
    }

    /// Close the SSH session and kill the subprocess.
    pub async fn close(mut self) -> Result<()> {
        if let Some(handle) = self.session.take() {
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        }
        let _ = self.child.kill().await;
        Ok(())
    }
}

/// Top-level: spawn gh, connect SSH, return CodespaceSsh.
pub async fn connect_codespace(
    codespace_name: &str,
    gh_bin: &str,
    timeout_secs: u64,
) -> Result<CodespaceSsh> {
    let _ = (codespace_name, gh_bin, timeout_secs);
    // TODO (Wave 4 subagent): implement
    Err(CodespaceError::Internal(
        "connect_codespace not yet implemented — Wave 4 subagent pending".into(),
    ))
}

/// Placeholder transport struct for type signatures.
pub struct SshTransport {
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
}
