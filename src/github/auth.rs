//! Token validation.

use super::GitHubClient;
use crate::{CodespaceError, Result};
use sha2::{Digest, Sha256};

/// Compute the token fingerprint: sha256(token)[:8]
/// Used for revocation detection in the state file. Never persist full token.
pub fn token_fingerprint(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let full = hex::encode(hasher.finalize());
    full[..8].to_string()
}

/// Resolve token from env var, then from token file, then error.
/// Env var takes precedence (so CI can override operator's local token).
pub fn resolve_token() -> Result<String> {
    // 1. Env var
    if let Ok(token) = std::env::var("CODESPACECTL_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }

    // 2. Token file
    let token_path = token_file_path();
    if token_path.exists() {
        check_token_file_perms(&token_path)?;
        let content = std::fs::read_to_string(&token_path)
            .map_err(|e| CodespaceError::Internal(format!("failed to read token file: {}", e)))?;
        let token = content.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }

    Err(CodespaceError::TokenMissing)
}

/// Token file location: `~/.config/codespacectl/token` (XDG on Linux,
/// `~/Library/Application Support/codespacectl/token` on macOS,
/// `%APPDATA%\codespacectl\token` on Windows).
pub fn token_file_path() -> std::path::PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp/.config"));
    config.join("codespacectl").join("token")
}

/// Save a token to the token file with 0600 perms.
pub fn save_token(token: &str) -> Result<()> {
    let path = token_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CodespaceError::Internal(format!("failed to create token dir: {}", e)))?;
    }
    std::fs::write(&path, token)
        .map_err(|e| CodespaceError::Internal(format!("failed to write token file: {}", e)))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            CodespaceError::Internal(format!("failed to set token file perms: {}", e))
        })?;
    }
    Ok(())
}

/// Clear the token file.
pub fn clear_token() -> Result<()> {
    let path = token_file_path();
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| CodespaceError::Internal(format!("failed to remove token file: {}", e)))?;
    }
    Ok(())
}

/// Verify the token file has 0600 perms on Unix (refuse to use if too open).
#[cfg(unix)]
fn check_token_file_perms(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path)
        .map_err(|e| CodespaceError::Internal(format!("token file stat failed: {}", e)))?;
    let mode = metadata.permissions().mode();
    if mode & 0o077 != 0 {
        return Err(CodespaceError::AuthFailed(format!(
            "token file {} has permissions {:o}, expected 0600 — refusing to use",
            path.display(),
            mode
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_token_file_perms(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

impl GitHubClient {
    /// Validate the token by calling `/user` and checking scopes.
    /// Returns the username if valid.
    pub async fn validate_token(&self) -> Result<String> {
        let resp = self.request(reqwest::Method::GET, "/user").send().await?;
        let resp = self.map_error(resp).await?;
        let parsed: serde_json::Value = resp.json().await?;
        let login = parsed
            .get("login")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CodespaceError::Internal("unexpected /user response: no login".into()))?
            .to_string();
        Ok(login)
    }

    /// Verify the token has `codespace` scope by listing codespaces.
    /// Returns true if the scope is present.
    pub async fn has_codespace_scope(&self) -> Result<bool> {
        match self.list_codespaces().await {
            Ok(_) => Ok(true),
            Err(CodespaceError::TokenInvalidScope { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    /// Serialize env-var-touching tests so they don't race each other on
    /// `CODESPACECTL_TOKEN` / `XDG_CONFIG_HOME`.
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_token_fingerprint_is_8_chars() {
        let fp = token_fingerprint("ghp_testtoken123");
        assert_eq!(fp.len(), 8);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_token_fingerprint_deterministic() {
        let fp1 = token_fingerprint("ghp_testtoken123");
        let fp2 = token_fingerprint("ghp_testtoken123");
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_token_fingerprint_differs_for_different_tokens() {
        let fp1 = token_fingerprint("ghp_token_a");
        let fp2 = token_fingerprint("ghp_token_b");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_token_fingerprint_handles_empty_string() {
        // sha256("")[:8] — must still be 8 hex chars.
        let fp = token_fingerprint("");
        assert_eq!(fp.len(), 8, "empty input should still produce 8 hex chars");
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_token_fingerprint_handles_unicode() {
        // sha256 of UTF-8 bytes of "αβγ" — must still be 8 hex chars.
        let fp = token_fingerprint("αβγ");
        assert_eq!(fp.len(), 8);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ---------------- resolve_token ----------------

    /// Helper: snapshot the current value of an env var (if set), set it to a
    /// new value, and return the original (Option).
    struct EnvGuard {
        key: &'static str,
        prev: Option<std::ffi::OsString>,
    }
    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let prev = std::env::var_os(key);
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
            Self { key, prev }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    /// Helper: redirect `token_file_path()` into a tempdir via XDG_CONFIG_HOME
    /// and snapshot/restore on drop.
    struct ConfigDirGuard {
        prev: Option<std::ffi::OsString>,
    }
    impl ConfigDirGuard {
        fn new(tmp: &std::path::Path) -> Self {
            let prev = std::env::var_os("XDG_CONFIG_HOME");
            std::env::set_var("XDG_CONFIG_HOME", tmp);
            Self { prev }
        }
    }
    impl Drop for ConfigDirGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    #[test]
    fn test_resolve_token_from_env_var() {
        let _g = env_lock().lock().unwrap();
        let _env = EnvGuard::set("CODESPACECTL_TOKEN", Some("ghp_env_token"));
        let token = resolve_token().expect("should resolve from env var");
        assert_eq!(token, "ghp_env_token");
    }

    #[test]
    fn test_resolve_token_falls_back_to_token_file() {
        let _g = env_lock().lock().unwrap();
        let _env = EnvGuard::set("CODESPACECTL_TOKEN", None);
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        // Write a token file under the redirected config dir.
        save_token("ghp_file_token").expect("save_token should succeed");

        let token = resolve_token().expect("should resolve from token file");
        assert_eq!(token, "ghp_file_token");
    }

    #[test]
    fn test_resolve_token_returns_token_missing_when_neither_set() {
        let _g = env_lock().lock().unwrap();
        let _env = EnvGuard::set("CODESPACECTL_TOKEN", None);
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        let err = resolve_token().unwrap_err();
        assert!(
            matches!(err, CodespaceError::TokenMissing),
            "expected TokenMissing when neither env var nor file exist, got {:?}",
            err
        );
    }

    #[test]
    fn test_resolve_token_returns_token_missing_when_env_empty() {
        let _g = env_lock().lock().unwrap();
        let _env = EnvGuard::set("CODESPACECTL_TOKEN", Some(""));
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        let err = resolve_token().unwrap_err();
        assert!(
            matches!(err, CodespaceError::TokenMissing),
            "empty env var + no file should map to TokenMissing, got {:?}",
            err
        );
    }

    #[test]
    fn test_resolve_token_prefers_env_over_file() {
        let _g = env_lock().lock().unwrap();
        let _env = EnvGuard::set("CODESPACECTL_TOKEN", Some("ghp_env_preferred"));
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        save_token("ghp_file_loses").expect("save_token should succeed");

        let token = resolve_token().expect("resolve_token should succeed");
        assert_eq!(
            token, "ghp_env_preferred",
            "env var should take precedence over token file"
        );
    }

    #[test]
    fn test_resolve_token_accepts_whitespace_env_var() {
        // Document behavior: `resolve_token` only checks `is_empty()`, so a
        // whitespace-only env var is returned as-is (Ok with whitespace).
        let _g = env_lock().lock().unwrap();
        let _env = EnvGuard::set("CODESPACECTL_TOKEN", Some("   "));
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        let token = resolve_token().expect("whitespace env var should resolve Ok");
        assert_eq!(token, "   ", "whitespace env var is returned as-is");
    }

    // ---------------- token_file_path ----------------

    #[test]
    fn test_token_file_path_ends_in_token() {
        let path = token_file_path();
        assert!(
            path.ends_with("token"),
            "token_file_path should end in 'token', got {}",
            path.display()
        );
    }

    #[test]
    fn test_token_file_path_is_under_config_dir() {
        let path = token_file_path();
        // The token file must live under a "codespacectl" subdir of the config dir.
        assert!(
            path.to_string_lossy().contains("codespacectl"),
            "token_file_path should be under a 'codespacectl' subdir, got {}",
            path.display()
        );
    }

    // ---------------- save_token ----------------

    #[test]
    fn test_save_token_writes_token_to_file() {
        let _g = env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        save_token("ghp_persisted").expect("save_token should succeed");

        let content = std::fs::read_to_string(token_file_path()).expect("file should exist");
        assert_eq!(content, "ghp_persisted");
    }

    #[test]
    fn test_save_token_creates_parent_dir_if_missing() {
        let _g = env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        // No codespacectl/ subdir exists yet — save_token should create it.
        save_token("ghp_token").expect("save_token should succeed");

        let path = token_file_path();
        assert!(path.exists(), "token file should exist after save");
        assert!(
            path.parent().unwrap().exists(),
            "parent dir should have been created"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_save_token_sets_0600_perms_on_unix() {
        let _g = env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        save_token("ghp_secret_perms").expect("save_token should succeed");

        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(token_file_path()).expect("metadata should exist");
        let mode = metadata.permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "token file should have 0600 perms, got {:o}",
            mode
        );
    }

    // ---------------- clear_token ----------------

    #[test]
    fn test_clear_token_removes_file() {
        let _g = env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        save_token("ghp_to_be_cleared").expect("save_token should succeed");
        assert!(
            token_file_path().exists(),
            "token file should exist before clear"
        );

        clear_token().expect("clear_token should succeed");
        assert!(
            !token_file_path().exists(),
            "token file should not exist after clear"
        );
    }

    #[test]
    fn test_clear_token_does_not_error_if_file_missing() {
        let _g = env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        assert!(
            !token_file_path().exists(),
            "precondition: token file should not exist"
        );
        // clear_token should return Ok even though the file doesn't exist.
        clear_token().expect("clear_token on missing file should return Ok");
    }

    // ---------------- check_token_file_perms (Unix only) ----------------

    #[cfg(unix)]
    #[test]
    fn test_check_token_file_perms_refuses_0644_file() {
        let _g = env_lock().lock().unwrap();
        let _env = EnvGuard::set("CODESPACECTL_TOKEN", None);
        let tmp = tempfile::tempdir().expect("tempdir");
        let _cfg = ConfigDirGuard::new(tmp.path());

        // Write a token file with 0644 perms (too open).
        let path = token_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&path, "ghp_insecure").expect("write");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("set perms");

        let err = resolve_token().unwrap_err();
        assert!(
            matches!(err, CodespaceError::AuthFailed(_)),
            "insecure perms should map to AuthFailed, got {:?}",
            err
        );
    }
}
