//! State file I/O — atomic writes, XDG-compliant paths.

use super::State;
use crate::{CodespaceError, Result};
use std::path::{Path, PathBuf};

/// Get the state directory (`~/.cache/codespacectl/` on Linux, platform equivalents).
pub fn state_dir() -> PathBuf {
    let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp/.cache"));
    cache.join("codespacectl")
}

/// Get the state file path (`~/.cache/codespacectl/state.json`).
pub fn state_file_path() -> PathBuf {
    state_dir().join("state.json")
}

/// Load state from disk. Returns default State if file doesn't exist.
pub fn load_state() -> Result<State> {
    let path = state_file_path();
    if !path.exists() {
        return Ok(State::default());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| {
        CodespaceError::Internal(format!("failed to read state file {}: {}", path.display(), e))
    })?;
    let state: State = serde_json::from_str(&content).map_err(|e| {
        CodespaceError::Internal(format!("failed to parse state file: {}", e))
    })?;
    Ok(state)
}

/// Save state to disk atomically (write to temp file, then rename).
pub fn save_state(state: &State) -> Result<()> {
    let dir = state_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        CodespaceError::Internal(format!("failed to create state dir {}: {}", dir.display(), e))
    })?;

    let path = state_file_path();
    let tmp_path = path.with_extension("json.tmp");

    let content = serde_json::to_string_pretty(state)?;
    std::fs::write(&tmp_path, content).map_err(|e| {
        CodespaceError::Internal(format!("failed to write temp state file: {}", e))
    })?;

    // Atomic rename
    std::fs::rename(&tmp_path, &path).map_err(|e| {
        CodespaceError::Internal(format!("failed to rename state file: {}", e))
    })?;

    // Set 0600 perms — only owner can read
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| CodespaceError::Internal(format!("failed to set state file perms: {}", e)))?;
    }

    Ok(())
}

/// Export state as a JSON string (for cross-machine transfer).
pub fn export_state() -> Result<String> {
    let state = load_state()?;
    Ok(serde_json::to_string_pretty(&state)?)
}

/// Import state from a JSON string (replaces existing state).
pub fn import_state(content: &str) -> Result<()> {
    let state: State = serde_json::from_str(content).map_err(|e| {
        CodespaceError::Internal(format!("failed to parse imported state: {}", e))
    })?;
    save_state(&state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_dir_is_absolute() {
        let dir = state_dir();
        assert!(dir.is_absolute(), "state dir must be absolute");
    }

    #[test]
    fn test_state_round_trip() {
        // Use a temp dir for the test
        let tmp = tempfile::tempdir().unwrap();
        let orig_cache = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("XDG_CACHE_HOME", tmp.path());

        let mut state = State::default();
        state.current_codespace = Some("test-codespace".into());
        save_state(&state).unwrap();

        let loaded = load_state().unwrap();
        assert_eq!(loaded.current_codespace, Some("test-codespace".to_string()));

        // Restore
        if let Some(v) = orig_cache {
            std::env::set_var("XDG_CACHE_HOME", v);
        } else {
            std::env::remove_var("XDG_CACHE_HOME");
        }
    }
}
