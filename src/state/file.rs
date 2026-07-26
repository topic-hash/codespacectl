//! State file I/O — atomic writes, XDG-compliant paths.

use super::State;
use crate::{CodespaceError, Result};
use std::path::PathBuf;

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
    use crate::state::codespace::CodespaceState;
    use crate::state::manifest_state::ManifestState;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Tests that touch `XDG_CACHE_HOME` are serialized via this lock because
    /// env vars are process-global. Avoids the `serial_test` crate dependency.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Run `body` with `XDG_CACHE_HOME` pointing at a fresh tempdir; restore
    /// prior value (or remove it) on exit.
    fn with_temp_xdg<F: FnOnce()>(body: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        let old_cache = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        body();
        match old_cache {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
    }

    #[test]
    fn test_state_dir_is_absolute() {
        let dir = state_dir();
        assert!(dir.is_absolute(), "state dir must be absolute");
    }

    #[test]
    fn test_state_dir_ends_with_codespacectl() {
        let dir = state_dir();
        let last = dir
            .file_name()
            .and_then(|s| s.to_str())
            .expect("state dir has a name");
        assert_eq!(
            last, "codespacectl",
            "state dir should end with 'codespacectl', got {:?}",
            dir
        );
    }

    #[test]
    fn test_state_file_path_ends_with_state_json() {
        let path = state_file_path();
        let last = path
            .file_name()
            .and_then(|s| s.to_str())
            .expect("state file path has a name");
        assert_eq!(last, "state.json");
    }

    #[test]
    fn test_state_file_path_is_absolute() {
        let path = state_file_path();
        assert!(path.is_absolute());
    }

    #[test]
    fn test_load_state_returns_default_when_file_missing() {
        with_temp_xdg(|| {
            // File doesn't exist in the tempdir, so load should return default.
            let state = load_state().expect("load should succeed");
            assert_eq!(state.version, 0);
            assert!(state.current_codespace.is_none());
            assert!(state.current_manifest.is_none());
            assert!(state.current_manifest_sha256.is_none());
            assert!(state.codespaces.is_empty());
            assert!(state.manifests.is_empty());
            assert!(state.token_fingerprint.is_none());
        });
    }

    #[test]
    fn test_save_state_creates_dir_if_missing() {
        with_temp_xdg(|| {
            // State dir doesn't exist yet.
            assert!(!state_dir().exists());
            let state = State::default();
            save_state(&state).expect("save should succeed");
            assert!(state_dir().exists(), "state dir should be created");
            assert!(state_file_path().exists(), "state file should be created");
        });
    }

    #[test]
    fn test_save_state_writes_valid_json() {
        with_temp_xdg(|| {
            let mut state = State::default();
            state.current_codespace = Some("test".into());
            save_state(&state).expect("save");
            let content = std::fs::read_to_string(state_file_path()).expect("read");
            // Should be valid JSON.
            let _: serde_json::Value =
                serde_json::from_str(&content).expect("file should contain valid JSON");
            // Should be pretty-printed (multi-line).
            assert!(content.contains('\n'), "state file should be pretty-printed");
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_save_state_sets_file_permissions_to_0600() {
        use std::os::unix::fs::PermissionsExt;
        with_temp_xdg(|| {
            let state = State::default();
            save_state(&state).expect("save");
            let meta = std::fs::metadata(state_file_path()).expect("metadata");
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "state file should have 0600 perms, got {:o}", mode);
        });
    }

    #[test]
    fn test_save_load_round_trips_all_fields() {
        with_temp_xdg(|| {
            let mut state = State::default();
            state.version = 1;
            state.current_codespace = Some("my-codespace".into());
            state.current_manifest = Some("/path/to/CODESPACE.yaml".into());
            state.current_manifest_sha256 =
                Some("abc123def456abc123def456abc123def456abc123def456abc123def456abcd".into());
            state.token_fingerprint = Some("deadbeef".into());

            let mut codespaces = HashMap::new();
            codespaces.insert(
                "my-codespace".into(),
                CodespaceState {
                    last_known_state: Some("Available".into()),
                    last_checked_at: Some("2024-01-01T00:00:00Z".into()),
                    created_at: Some("2024-01-01T00:00:00Z".into()),
                    host_key_fingerprint: Some("SHA256:xyz".into()),
                    host_key_stored_at: Some("2024-01-01T00:00:00Z".into()),
                    last_health_status: Some("green".into()),
                    last_health_checked_at: Some("2024-01-01T00:00:00Z".into()),
                },
            );
            state.codespaces = codespaces;

            let mut manifests = HashMap::new();
            manifests.insert(
                "/path/to/CODESPACE.yaml".into(),
                ManifestState {
                    sha256: Some("abc123".into()),
                    last_validated_at: Some("2024-01-01T00:00:00Z".into()),
                },
            );
            state.manifests = manifests;

            save_state(&state).expect("save");
            let loaded = load_state().expect("load");

            assert_eq!(loaded.version, 1);
            assert_eq!(loaded.current_codespace.as_deref(), Some("my-codespace"));
            assert_eq!(loaded.current_manifest.as_deref(), Some("/path/to/CODESPACE.yaml"));
            assert_eq!(
                loaded.current_manifest_sha256.as_deref(),
                Some("abc123def456abc123def456abc123def456abc123def456abc123def456abcd")
            );
            assert_eq!(loaded.token_fingerprint.as_deref(), Some("deadbeef"));
            assert_eq!(loaded.codespaces.len(), 1);
            let cs = loaded.codespaces.get("my-codespace").expect("codespace entry");
            assert_eq!(cs.last_known_state.as_deref(), Some("Available"));
            assert_eq!(cs.host_key_fingerprint.as_deref(), Some("SHA256:xyz"));
            assert_eq!(loaded.manifests.len(), 1);
            let ms = loaded
                .manifests
                .get("/path/to/CODESPACE.yaml")
                .expect("manifest entry");
            assert_eq!(ms.sha256.as_deref(), Some("abc123"));
        });
    }

    #[test]
    fn test_save_state_is_atomic_temp_file_cleaned_up() {
        with_temp_xdg(|| {
            let state = State::default();
            save_state(&state).expect("save");
            // The temp file should NOT exist after the atomic rename.
            let tmp = state_file_path().with_extension("json.tmp");
            assert!(
                !tmp.exists(),
                "temp file {:?} should not exist after successful save",
                tmp
            );
            // Final file should exist.
            assert!(state_file_path().exists());
        });
    }

    #[test]
    fn test_export_state_returns_valid_json_string() {
        with_temp_xdg(|| {
            let mut state = State::default();
            state.current_codespace = Some("export-test".into());
            save_state(&state).expect("save");
            let json = export_state().expect("export");
            let parsed: serde_json::Value =
                serde_json::from_str(&json).expect("export should produce valid JSON");
            assert_eq!(parsed["current_codespace"], "export-test");
        });
    }

    #[test]
    fn test_export_state_pretty_prints() {
        with_temp_xdg(|| {
            save_state(&State::default()).expect("save");
            let json = export_state().expect("export");
            assert!(json.contains('\n'), "export should be pretty-printed");
        });
    }

    #[test]
    fn test_import_state_replaces_existing_state() {
        with_temp_xdg(|| {
            // First, write a state with current_codespace = "before".
            let mut state = State::default();
            state.current_codespace = Some("before".into());
            save_state(&state).expect("save");
            // Now import a different state.
            let json = serde_json::json!({
                "version": 1,
                "current_codespace": "after",
            })
            .to_string();
            import_state(&json).expect("import");
            let loaded = load_state().expect("load");
            assert_eq!(loaded.current_codespace.as_deref(), Some("after"));
            assert_eq!(loaded.version, 1);
        });
    }

    #[test]
    fn test_import_state_errors_on_invalid_json() {
        with_temp_xdg(|| {
            let err = import_state("not valid json").unwrap_err();
            assert_eq!(err.kind(), "internal_error");
            assert!(err.to_string().contains("failed to parse"));
        });
    }

    #[test]
    fn test_import_state_errors_on_invalid_state_schema() {
        with_temp_xdg(|| {
            // Valid JSON but missing required fields would still parse with
            // `#[serde(default)]` everywhere; let's test a structurally-broken
            // payload: apiVersion string where a struct is expected.
            let err = import_state("\"just-a-string\"").unwrap_err();
            assert_eq!(err.kind(), "internal_error");
        });
    }

    #[test]
    fn test_import_state_then_load_round_trip() {
        with_temp_xdg(|| {
            let original = State {
                version: 7,
                current_codespace: Some("rt".into()),
                current_manifest: Some("/p".into()),
                current_manifest_sha256: Some("sha".into()),
                codespaces: HashMap::new(),
                manifests: HashMap::new(),
                token_fingerprint: Some("fp".into()),
            };
            let json = serde_json::to_string(&original).expect("serialize");
            import_state(&json).expect("import");
            let loaded = load_state().expect("load");
            assert_eq!(loaded.version, 7);
            assert_eq!(loaded.current_codespace.as_deref(), Some("rt"));
            assert_eq!(loaded.current_manifest.as_deref(), Some("/p"));
            assert_eq!(loaded.current_manifest_sha256.as_deref(), Some("sha"));
            assert_eq!(loaded.token_fingerprint.as_deref(), Some("fp"));
        });
    }

    #[test]
    fn test_load_state_errors_on_corrupted_file() {
        with_temp_xdg(|| {
            save_state(&State::default()).expect("save creates dir");
            std::fs::write(state_file_path(), "not valid json {{{").expect("write garbage");
            let err = load_state().unwrap_err();
            assert_eq!(err.kind(), "internal_error");
            assert!(err.to_string().contains("failed to parse"));
        });
    }

    #[test]
    fn test_load_state_errors_on_unreadable_file_content() {
        with_temp_xdg(|| {
            save_state(&State::default()).expect("save");
            // Write garbage that's not even UTF-8.
            std::fs::write(state_file_path(), b"\xff\xfe\x00invalid").expect("write bytes");
            let err = load_state().unwrap_err();
            assert_eq!(err.kind(), "internal_error");
        });
    }

    #[test]
    fn test_concurrent_save_state_calls_dont_corrupt_file() {
        use std::thread;

        with_temp_xdg(|| {
            // Pre-create the dir so all threads race on the file write, not dir.
            save_state(&State::default()).expect("initial save");

            // Spawn 8 threads each trying to save a different state. The
            // current `save_state` implementation writes to a fixed tmp path
            // (`state.json.tmp`) and renames — it is NOT designed for
            // concurrent writers, so some threads may fail with a "rename:
            // No such file" error when their tmp file was already renamed
            // out by another thread. That's a known limitation, but the
            // important property is: the FINAL `state.json` on disk must
            // always be a valid, parseable State — never a half-written mix.
            let threads: Vec<_> = (0..8)
                .map(|i| {
                    thread::spawn(move || {
                        let mut state = State::default();
                        state.current_codespace = Some(format!("cs-{}", i));
                        // Ignore failures (some threads will lose the race).
                        let _ = save_state(&state);
                    })
                })
                .collect();
            for t in threads {
                t.join().expect("thread should not panic");
            }

            // Whatever the final state is, the file should parse cleanly.
            // This is the key correctness property — no torn writes.
            let loaded = load_state().expect("load should succeed after concurrent writes");
            if let Some(cs) = loaded.current_codespace.as_deref() {
                assert!(
                    cs.starts_with("cs-"),
                    "expected cs-N pattern, got: {}",
                    cs
                );
            }
            // File should still be valid JSON.
            let content = std::fs::read_to_string(state_file_path()).expect("read file");
            let _: serde_json::Value =
                serde_json::from_str(&content).expect("file should be valid JSON after concurrent writes");
        });
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
