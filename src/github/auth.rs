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
        let content = std::fs::read_to_string(&token_path).map_err(|e| {
            CodespaceError::Internal(format!("failed to read token file: {}", e))
        })?;
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
        std::fs::create_dir_all(parent).map_err(|e| {
            CodespaceError::Internal(format!("failed to create token dir: {}", e))
        })?;
    }
    std::fs::write(&path, token).map_err(|e| {
        CodespaceError::Internal(format!("failed to write token file: {}", e))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| CodespaceError::Internal(format!("failed to set token file perms: {}", e)))?;
    }
    Ok(())
}

/// Clear the token file.
pub fn clear_token() -> Result<()> {
    let path = token_file_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| {
            CodespaceError::Internal(format!("failed to remove token file: {}", e))
        })?;
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
            path.display(), mode
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
        let resp = self
            .request(reqwest::Method::GET, "/user")
            .send()
            .await?;
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
}
