//! Secret storage at `~/.cache/codespacectl/secrets/<name>.age`.
//!
//! TODO (Wave 4 subagent): implement with `age` crate.
//! For now, plaintext with 0600 perms + warning (acceptable fallback per design).

use crate::{CodespaceError, Result};
use std::path::PathBuf;

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

/// Secret store — encrypts secrets at rest with age, falls back to plaintext+0600.
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
            // TODO (Wave 4 subagent): generate age identity via `age::x25519::Identity::generate()`
            // and write to identity_path with 0600 perms.
        }

        Ok(())
    }

    /// Store a secret (encrypted with the local age identity).
    pub fn set(name: &str, value: &str) -> Result<()> {
        let _ = (name, value);
        // TODO (Wave 4 subagent): implement age encryption
        Err(CodespaceError::Internal(
            "SecretStore::set not yet implemented — Wave 4 subagent pending".into(),
        ))
    }

    /// Retrieve a secret (decrypt with the local age identity).
    pub fn get(name: &str) -> Result<String> {
        let _ = name;
        // TODO (Wave 4 subagent): implement age decryption
        Err(CodespaceError::Internal(
            "SecretStore::get not yet implemented — Wave 4 subagent pending".into(),
        ))
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
