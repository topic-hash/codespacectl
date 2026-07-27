//! Secret storage at `~/.cache/codespacectl/secrets/<name>.bin`.
//!
//! Uses AES-256-GCM for authenticated encryption at rest. The 32-byte key is
//! stored at `~/.config/codespacectl/key.bin` (0600 perms on Unix). Each secret
//! file contains a 12-byte nonce followed by the ciphertext + GCM tag.
//!
//! Replaces the previous `age`-based implementation to avoid the transitive
//! dependency chain `age → i18n-embed-fl → proc-macro-error2 v2.0.1` which is
//! flagged as future-incompatible by the Rust compiler.

use crate::{CodespaceError, Result};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::RngCore;
use std::path::PathBuf;

/// Get the directory where secrets are stored.
pub fn secrets_dir() -> PathBuf {
    let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp/.cache"));
    cache.join("codespacectl").join("secrets")
}

/// Path to a specific secret file.
pub fn secret_path(name: &str) -> PathBuf {
    secrets_dir().join(format!("{}.bin", name))
}

/// Path to the master encryption key file (~/.config/codespacectl/key.bin).
/// 32 random bytes, 0600 perms on Unix. Generated on first use via `init()`.
pub fn identity_path() -> PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("/tmp/.config"));
    config.join("codespacectl").join("key.bin")
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

/// Secret store — encrypts secrets at rest with AES-256-GCM using a local
/// 32-byte key. Key is generated on first `init()` call and persisted at
/// `~/.config/codespacectl/key.bin` with 0600 perms.
pub struct SecretStore;

impl SecretStore {
    /// Initialize: ensure secrets dir exists, generate master key if missing.
    pub fn init() -> Result<()> {
        let dir = secrets_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            CodespaceError::Internal(format!("failed to create secrets dir: {}", e))
        })?;

        let key_path = identity_path();
        if !key_path.exists() {
            if let Some(parent) = key_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CodespaceError::Internal(format!("failed to create key dir: {}", e))
                })?;
            }
            // Generate 32 random bytes for AES-256
            let mut key_bytes = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut key_bytes);
            std::fs::write(&key_path, key_bytes).map_err(|e| {
                CodespaceError::Internal(format!("failed to write key file: {}", e))
            })?;
            set_owner_only_permissions(&key_path)?;
        }

        Ok(())
    }

    /// Store a secret (encrypted with AES-256-GCM).
    pub fn set(name: &str, value: &str) -> Result<()> {
        Self::init()?;
        let key = load_master_key()?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| CodespaceError::Internal(format!("AES key init failed: {}", e)))?;

        // Generate a random 12-byte nonce (GCM standard nonce size)
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt: output is ciphertext + GCM tag (16 bytes appended by aes-gcm)
        let ciphertext = cipher
            .encrypt(nonce, value.as_bytes())
            .map_err(|e| SecretError::EncryptFailed(e.to_string()))?;

        // Write: [12-byte nonce][ciphertext + tag]
        let mut output = Vec::with_capacity(12 + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        let path = secret_path(name);
        std::fs::write(&path, &output)?;
        set_owner_only_permissions(&path)?;
        Ok(())
    }

    /// Retrieve a secret (decrypt with AES-256-GCM).
    pub fn get(name: &str) -> Result<String> {
        let path = secret_path(name);
        if !path.exists() {
            return Err(SecretError::NotFound(name.to_string()).into());
        }

        let key = load_master_key()?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| CodespaceError::Internal(format!("AES key init failed: {}", e)))?;

        let file_content = std::fs::read(&path)?;
        if file_content.len() < 12 {
            return Err(SecretError::DecryptFailed(format!(
                "secret file too short: {} bytes (need >= 12 for nonce)",
                file_content.len()
            ))
            .into());
        }

        // Split nonce + ciphertext
        let (nonce_bytes, ciphertext) = file_content.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| SecretError::DecryptFailed(e.to_string()))?;

        let value = String::from_utf8(plaintext)
            .map_err(|e| SecretError::DecryptFailed(format!("plaintext not valid UTF-8: {}", e)))?;

        Ok(value)
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

/// Load the 32-byte master key from the identity file.
fn load_master_key() -> Result<[u8; 32]> {
    let path = identity_path();
    if !path.exists() {
        return Err(CodespaceError::Internal(
            "master key not initialized — call SecretStore::init() first".into(),
        ));
    }
    let content = std::fs::read(&path)?;
    if content.len() != 32 {
        return Err(CodespaceError::Internal(format!(
            "master key file is {} bytes, expected 32",
            content.len()
        )));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&content);
    Ok(key)
}

/// Set file permissions to 0600 (owner read/write only) on Unix.
#[cfg(unix)]
fn set_owner_only_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
        CodespaceError::Internal(format!("failed to set perms on {}: {}", path.display(), e))
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &std::path::Path) -> Result<()> {
    // No-op on non-Unix — Windows ACLs are more complex and not handled here.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that touch env vars (XDG_CACHE_HOME, XDG_CONFIG_HOME).
    /// Without this, parallel test runs race on the global env.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_dirs() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        std::sync::MutexGuard<'static, ()>,
    ) {
        let _guard = ENV_LOCK.lock().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let prev_cache = std::env::var_os("XDG_CACHE_HOME");
        let prev_config = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CACHE_HOME", cache.path());
        std::env::set_var("XDG_CONFIG_HOME", config.path());
        // We can't restore env vars after because TempDir is still alive —
        // but the ENV_LOCK ensures no other test runs concurrently.
        let _ = (prev_cache, prev_config);
        (cache, config, _guard)
    }

    #[test]
    fn test_secret_store_init_creates_dirs() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::init().unwrap();
        assert!(secrets_dir().exists(), "secrets dir should exist");
        assert!(
            identity_path().parent().unwrap().exists(),
            "config dir should exist"
        );
    }

    #[test]
    fn test_secret_store_init_generates_key() {
        let (_cache, _config, _guard) = with_temp_dirs();
        assert!(
            !identity_path().exists(),
            "key should not exist before init"
        );
        SecretStore::init().unwrap();
        assert!(identity_path().exists(), "key should exist after init");
        let key = std::fs::read(identity_path()).unwrap();
        assert_eq!(key.len(), 32, "master key must be 32 bytes for AES-256");
    }

    #[test]
    fn test_secret_store_init_idempotent() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::init().unwrap();
        let key1 = std::fs::read(identity_path()).unwrap();
        SecretStore::init().unwrap();
        let key2 = std::fs::read(identity_path()).unwrap();
        assert_eq!(key1, key2, "calling init twice must NOT regenerate the key");
    }

    #[test]
    fn test_secret_round_trip_simple() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("test", "hello world").unwrap();
        let retrieved = SecretStore::get("test").unwrap();
        assert_eq!(retrieved, "hello world");
    }

    #[test]
    fn test_secret_round_trip_long_string() {
        let (_cache, _config, _guard) = with_temp_dirs();
        let value = "x".repeat(10_000);
        SecretStore::set("big", &value).unwrap();
        let retrieved = SecretStore::get("big").unwrap();
        assert_eq!(retrieved.len(), 10_000);
        assert_eq!(retrieved, value);
    }

    #[test]
    fn test_secret_round_trip_unicode() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("unicode", "Hello 世界 🌍café").unwrap();
        let retrieved = SecretStore::get("unicode").unwrap();
        assert_eq!(retrieved, "Hello 世界 🌍café");
    }

    #[test]
    fn test_secret_round_trip_newlines() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("multiline", "line1\nline2\nline3\n").unwrap();
        let retrieved = SecretStore::get("multiline").unwrap();
        assert_eq!(retrieved, "line1\nline2\nline3\n");
    }

    #[test]
    fn test_secret_get_missing_returns_not_found() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::init().unwrap();
        let err = SecretStore::get("nonexistent").unwrap_err();
        assert!(
            matches!(err, CodespaceError::Internal(ref msg) if msg.contains("secret not found")),
            "expected NotFound error, got: {:?}",
            err
        );
    }

    #[test]
    fn test_secret_exists_before_and_after_set() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::init().unwrap();
        assert!(!SecretStore::exists("foo"));
        SecretStore::set("foo", "bar").unwrap();
        assert!(SecretStore::exists("foo"));
    }

    #[test]
    fn test_secret_delete_removes_file() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("temp", "value").unwrap();
        assert!(SecretStore::exists("temp"));
        SecretStore::delete("temp").unwrap();
        assert!(!SecretStore::exists("temp"));
    }

    #[test]
    fn test_secret_delete_nonexistent_doesnt_error() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::init().unwrap();
        // Should not error — delete on non-existent is idempotent
        assert!(SecretStore::delete("never-existed").is_ok());
    }

    #[test]
    fn test_secret_overwrite() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("key", "old").unwrap();
        SecretStore::set("key", "new").unwrap();
        let retrieved = SecretStore::get("key").unwrap();
        assert_eq!(retrieved, "new", "overwrite should replace value");
    }

    #[test]
    fn test_two_secrets_both_retrieve() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("a", "value_a").unwrap();
        SecretStore::set("b", "value_b").unwrap();
        assert_eq!(SecretStore::get("a").unwrap(), "value_a");
        assert_eq!(SecretStore::get("b").unwrap(), "value_b");
    }

    #[cfg(unix)]
    #[test]
    fn test_identity_file_perms() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::init().unwrap();
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(identity_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            perms & 0o777,
            0o600,
            "identity file must be 0600, got {:o}",
            perms
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_secret_file_perms() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("foo", "bar").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(secret_path("foo"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            perms & 0o777,
            0o600,
            "secret file must be 0600, got {:o}",
            perms
        );
    }

    #[test]
    fn test_secret_path_ends_with_bin() {
        let path = secret_path("test");
        assert_eq!(path.extension().unwrap(), "bin");
    }

    #[test]
    fn test_identity_path_under_config_dir() {
        let path = identity_path();
        assert!(path.to_string_lossy().contains("codespacectl"));
        assert!(path.to_string_lossy().ends_with("key.bin"));
    }

    #[test]
    fn test_ciphertext_format_nonce_plus_ciphertext() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("format_test", "value").unwrap();
        let content = std::fs::read(secret_path("format_test")).unwrap();
        // Format: 12-byte nonce + ciphertext (which includes 16-byte GCM tag)
        assert!(
            content.len() >= 12 + 16,
            "file must be at least 28 bytes (nonce + tag)"
        );
        assert!(
            content.len() <= 12 + 16 + 100,
            "file should not be excessively large for short value"
        );
    }

    #[test]
    fn test_decryption_with_wrong_key_fails() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("tamper_test", "secret_value").unwrap();

        // Corrupt the master key — decryption should fail
        let key_path = identity_path();
        let mut bad_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bad_key);
        std::fs::write(&key_path, bad_key).unwrap();

        let result = SecretStore::get("tamper_test");
        assert!(result.is_err(), "decryption with wrong key should fail");
    }

    #[test]
    fn test_decryption_with_corrupted_ciphertext_fails() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("corrupt_test", "secret_value").unwrap();

        // Corrupt the ciphertext (flip a bit after the nonce)
        let path = secret_path("corrupt_test");
        let mut content = std::fs::read(&path).unwrap();
        content[20] ^= 0xFF; // flip a byte in the ciphertext portion
        std::fs::write(&path, &content).unwrap();

        let result = SecretStore::get("corrupt_test");
        assert!(
            result.is_err(),
            "decryption with corrupted ciphertext should fail"
        );
    }

    #[test]
    fn test_decryption_with_truncated_file_fails() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::init().unwrap();
        // Write a file too short to contain a nonce
        std::fs::write(secret_path("short"), b"tiny").unwrap();
        let result = SecretStore::get("short");
        assert!(result.is_err(), "decryption of truncated file should fail");
    }

    #[test]
    fn test_empty_value_round_trips() {
        let (_cache, _config, _guard) = with_temp_dirs();
        SecretStore::set("empty", "").unwrap();
        let retrieved = SecretStore::get("empty").unwrap();
        assert_eq!(retrieved, "");
    }
}
