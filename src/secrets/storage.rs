//! Secret storage at `~/.cache/codespacectl/secrets/<name>.age`.
//!
//! Secrets are encrypted at rest with the `age` crate using an X25519
//! identity stored at `~/.config/codespacectl/identity.age`. The identity
//! is generated lazily on first use. All secret files (and the identity
//! file itself) are written with `0600` permissions on Unix.

use crate::{CodespaceError, Result};
use age::secrecy::ExposeSecret;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Get the directory where secrets are stored.
pub fn secrets_dir() -> PathBuf {
    let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp/.cache"));
    cache.join("codespacectl").join("secrets")
}

/// Path to a specific secret file.
pub fn secret_path(name: &str) -> PathBuf {
    secrets_dir().join(format!("{}.age", name))
}

/// Path to the age identity file (used for encryption/decryption).
pub fn identity_path() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("/tmp/.config"));
    config.join("codespacectl").join("identity.age")
}

/// Errors from secret operations.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret not found: {0}")]
    NotFound(String),
    #[error("secret encryption failed: {0}")]
    EncryptFailed(String),
    #[error("secret decryption failed: {0}")]
    DecryptFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<SecretError> for CodespaceError {
    fn from(e: SecretError) -> Self {
        CodespaceError::Internal(format!("secret error: {}", e))
    }
}

/// Set file permissions to 0600 on Unix; no-op elsewhere.
fn set_owner_only_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Secret store — encrypts secrets at rest with age X25519 identities.
pub struct SecretStore;

impl SecretStore {
    /// Initialize: ensure secrets dir exists, generate age identity if missing.
    pub fn init() -> Result<()> {
        let dir = secrets_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            CodespaceError::Internal(format!("failed to create secrets dir: {}", e))
        })?;

        let identity = identity_path();
        if !identity.exists() {
            if let Some(parent) = identity.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CodespaceError::Internal(format!("failed to create identity dir: {}", e))
                })?;
            }
            // Generate a fresh X25519 age identity and persist it with 0600 perms.
            let id = age::x25519::Identity::generate();
            let secret_str = id.to_string();
            let plaintext = secret_str.expose_secret();
            // Write to a temp file in the same dir, then atomically set perms + rename
            // so we never leave a world-readable identity on disk.
            std::fs::write(&identity, plaintext.as_bytes())?;
            set_owner_only_permissions(&identity)
                .map_err(|e| SecretError::Io(e))?;
        }

        Ok(())
    }

    /// Load the age identity from disk. Caller must have called `init()` first.
    fn load_identity() -> std::result::Result<age::x25519::Identity, SecretError> {
        let content = std::fs::read_to_string(identity_path())?;
        age::x25519::Identity::from_str(content.trim())
            .map_err(|e| SecretError::EncryptFailed(format!("invalid identity: {}", e)))
    }

    /// Store a secret (encrypted with the local age identity).
    pub fn set(name: &str, value: &str) -> Result<()> {
        Self::init()?;
        let identity = Self::load_identity()?;
        let recipient = identity.to_public();

        let encryptor = age::Encryptor::with_recipients(std::iter::once(&recipient as _))
            .map_err(|e| SecretError::EncryptFailed(e.to_string()))?;

        // Double-wrap: outer = ASCII armor, inner = age stream encryption.
        let mut output: Vec<u8> = Vec::new();
        let armored = age::armor::ArmoredWriter::wrap_output(
            &mut output,
            age::armor::Format::AsciiArmor,
        )?;
        let mut writer = encryptor.wrap_output(armored)?;
        writer.write_all(value.as_bytes())?;
        // finish() on the StreamWriter flushes the age stream and returns the
        // underlying ArmoredWriter; finish() on that writes the armor end marker
        // and returns the inner writer (which we drop).
        writer.finish()?.finish()?;

        let path = secret_path(name);
        std::fs::write(&path, &output)?;
        set_owner_only_permissions(&path)?;

        Ok(())
    }

    /// Retrieve a secret (decrypt with the local age identity).
    pub fn get(name: &str) -> Result<String> {
        let path = secret_path(name);
        if !path.exists() {
            return Err(SecretError::NotFound(name.to_string()).into());
        }
        let encrypted_blob = std::fs::read(&path)?;
        let identity = Self::load_identity()
            .map_err(|e| match e {
                SecretError::EncryptFailed(msg) => SecretError::DecryptFailed(msg),
                other => other,
            })?;

        // We wrote ASCII-armored blobs, so wrap with ArmoredReader before parsing.
        let decryptor =
            age::Decryptor::new_buffered(age::armor::ArmoredReader::new(&encrypted_blob[..]))
                .map_err(|e| SecretError::DecryptFailed(e.to_string()))?;
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as _))
            .map_err(|e| SecretError::DecryptFailed(e.to_string()))?;
        let mut decrypted = String::new();
        reader
            .read_to_string(&mut decrypted)
            .map_err(|e| SecretError::DecryptFailed(e.to_string()))?;

        // Trim a single trailing newline if present (defensive; we never write one).
        if decrypted.ends_with('\n') {
            decrypted.truncate(decrypted.len() - 1);
        }
        Ok(decrypted)
    }

    /// Check if a secret exists.
    pub fn exists(name: &str) -> bool {
        secret_path(name).exists()
    }

    /// Delete a secret.
    pub fn delete(name: &str) -> Result<()> {
        let path = secret_path(name);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that touch `XDG_CACHE_HOME` / `XDG_CONFIG_HOME` env vars are
    /// serialized via this lock to avoid cross-test interference (env vars
    /// are process-global). We avoid adding `serial_test` to deps.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Run `body` with `XDG_CACHE_HOME` and `XDG_CONFIG_HOME` pointing at a
    /// fresh tempdir; restores prior values (or removes them) afterwards.
    fn with_temp_xdg<F: FnOnce()>(body: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");

        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        let old_config = std::env::var_os("XDG_CONFIG_HOME");

        std::env::set_var("XDG_CACHE_HOME", dir.path());
        std::env::set_var("XDG_CONFIG_HOME", dir.path());

        body();

        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        match old_config {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn test_secret_store_init_creates_dirs() {
        with_temp_xdg(|| {
            SecretStore::init().expect("init should succeed");
            assert!(secrets_dir().exists(), "secrets dir should exist");
            assert!(
                identity_path().parent().unwrap().exists(),
                "identity parent dir should exist"
            );
            assert!(identity_path().exists(), "identity file should exist");
        });
    }

    #[test]
    fn test_secret_round_trip() {
        with_temp_xdg(|| {
            SecretStore::init().unwrap();
            let name = "test-rt";
            let value = "super-secret-value-12345";
            SecretStore::set(name, value).expect("set should succeed");
            let got = SecretStore::get(name).expect("get should succeed");
            assert_eq!(got, value);
        });
    }

    #[test]
    fn test_secret_get_missing_returns_error() {
        with_temp_xdg(|| {
            SecretStore::init().unwrap();
            let err = SecretStore::get("does-not-exist").unwrap_err();
            // Errors flow through CodespaceError::Internal; check the message.
            let msg = err.to_string();
            assert!(
                msg.contains("not found"),
                "expected NotFound error, got: {}",
                msg
            );
        });
    }

    #[test]
    fn test_secret_overwrite() {
        with_temp_xdg(|| {
            SecretStore::init().unwrap();
            let name = "ovr";
            SecretStore::set(name, "first").unwrap();
            SecretStore::set(name, "second").unwrap();
            let got = SecretStore::get(name).unwrap();
            assert_eq!(got, "second");
        });
    }

    #[test]
    fn test_secret_exists() {
        with_temp_xdg(|| {
            SecretStore::init().unwrap();
            let name = "ex";
            assert!(!SecretStore::exists(name));
            SecretStore::set(name, "v").unwrap();
            assert!(SecretStore::exists(name));
            SecretStore::delete(name).unwrap();
            assert!(!SecretStore::exists(name));
        });
    }

    #[test]
    fn test_secret_delete() {
        with_temp_xdg(|| {
            SecretStore::init().unwrap();
            let name = "del";
            SecretStore::set(name, "v").unwrap();
            assert!(secret_path(name).exists());
            SecretStore::delete(name).unwrap();
            assert!(!secret_path(name).exists());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_identity_file_perms() {
        use std::os::unix::fs::PermissionsExt;
        with_temp_xdg(|| {
            SecretStore::init().unwrap();
            let meta = std::fs::metadata(identity_path()).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(
                mode,
                0o600,
                "identity file should have 0600 perms, got {:o}",
                mode
            );
        });
    }
}
