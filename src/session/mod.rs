//! Session logging: append-only NDJSON at `~/.cache/codespacectl/sessions/<uuid>.ndjson`.
//!
//! Wave 4 subagent (parallel): implement.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One entry in a session log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub timestamp: String,
    pub kind: SessionEntryKind,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEntryKind {
    Connect,
    ExecStart,
    ExecOutput,
    ExecEnd,
    HealthCheck,
    Hook,
    Stop,
    Warning,
    Error,
}

/// A session — one logical "connect → do stuff → stop" sequence.
pub struct SessionLog {
    pub id: String,
    pub path: PathBuf,
}

impl SessionLog {
    /// Start a new session, returning the log writer.
    pub fn new(codespace_name: &str, manifest_name: &str) -> Result<Self> {
        let id = uuid::Uuid::new_v4().to_string();
        let path = sessions_dir().join(format!("{}.ndjson", id));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Write header
        let header = SessionEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            kind: SessionEntryKind::Connect,
            data: serde_json::json!({
                "codespace": codespace_name,
                "manifest": manifest_name,
            }),
        };
        Self::append_entry(&path, &header)?;
        Ok(Self { id, path })
    }

    /// Append an entry to the session log.
    pub fn append(&self, kind: SessionEntryKind, data: serde_json::Value) -> Result<()> {
        let entry = SessionEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            kind,
            data,
        };
        Self::append_entry(&self.path, &entry)
    }

    fn append_entry(path: &std::path::Path, entry: &SessionEntry) -> Result<()> {
        let line = serde_json::to_string(entry)?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    /// Get the log file path (for the JSON envelope's session.logPath field).
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Get the session ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Read a session log by ID.
    pub fn read(id: &str) -> Result<Vec<SessionEntry>> {
        let path = sessions_dir().join(format!("{}.ndjson", id));
        if !path.exists() {
            return Ok(vec![]);
        }
        let content = std::fs::read_to_string(&path)?;
        let mut entries = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: SessionEntry = serde_json::from_str(line)?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// List recent session IDs (most recent first).
    pub fn list_recent(n: usize) -> Result<Vec<(String, std::time::SystemTime)>> {
        let dir = sessions_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut sessions: Vec<(String, std::time::SystemTime)> = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ndjson") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let modified = entry.metadata()?.modified()?;
            sessions.push((stem, modified));
        }
        sessions.sort_by(|a, b| b.1.cmp(&a.1));
        sessions.truncate(n);
        Ok(sessions)
    }
}

fn sessions_dir() -> PathBuf {
    let cache = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp/.cache"));
    cache.join("codespacectl").join("sessions")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that touch `XDG_CACHE_HOME` are serialized via this lock because
    /// env vars are process-global. The original session module doesn't have
    /// its own lock, so all session tests coordinate through this one.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Run `body` with `XDG_CACHE_HOME` pointing at a fresh tempdir; restore
    /// the prior value (or remove it) on exit.
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
    fn test_session_log_new_creates_session_file() {
        with_temp_xdg(|| {
            let log = SessionLog::new("my-codespace", "my-app").expect("create session");
            assert!(log.path.exists(), "session file should be created");
            assert!(log.path.is_file());
        });
    }

    #[test]
    fn test_session_log_new_writes_connect_entry_as_first_line() {
        with_temp_xdg(|| {
            let log = SessionLog::new("my-cs", "my-app").expect("create");
            let entries = SessionLog::read(&log.id).expect("read");
            assert!(!entries.is_empty(), "should have at least one entry");
            assert_eq!(
                entries.len(),
                1,
                "should have exactly one entry on creation"
            );
            let first = &entries[0];
            assert!(matches!(first.kind, SessionEntryKind::Connect));
            assert_eq!(first.data["codespace"], "my-cs");
            assert_eq!(first.data["manifest"], "my-app");
        });
    }

    #[test]
    fn test_session_log_append_adds_entries_in_order() {
        with_temp_xdg(|| {
            let log = SessionLog::new("cs", "app").expect("create");
            log.append(
                SessionEntryKind::ExecStart,
                serde_json::json!({"cmd": "first"}),
            )
            .expect("append");
            log.append(
                SessionEntryKind::ExecEnd,
                serde_json::json!({"cmd": "second", "exit": 0}),
            )
            .expect("append");
            let entries = SessionLog::read(&log.id).expect("read");
            // 1 Connect + 2 appends = 3 entries.
            assert_eq!(entries.len(), 3);
            assert!(matches!(entries[0].kind, SessionEntryKind::Connect));
            assert!(matches!(entries[1].kind, SessionEntryKind::ExecStart));
            assert!(matches!(entries[2].kind, SessionEntryKind::ExecEnd));
            assert_eq!(entries[1].data["cmd"], "first");
            assert_eq!(entries[2].data["exit"], 0);
        });
    }

    #[test]
    fn test_session_log_append_does_not_overwrite_previous_entries() {
        with_temp_xdg(|| {
            let log = SessionLog::new("cs", "app").expect("create");
            // Append many entries.
            for i in 0..5 {
                log.append(SessionEntryKind::ExecStart, serde_json::json!({"i": i}))
                    .expect("append");
            }
            let entries = SessionLog::read(&log.id).expect("read");
            assert_eq!(entries.len(), 6, "1 Connect + 5 appends");
            // Each value should appear in order.
            for (i, entry) in entries.iter().skip(1).enumerate() {
                assert_eq!(entry.data["i"], i as u64);
            }
        });
    }

    #[test]
    fn test_session_log_read_returns_entries_for_session() {
        with_temp_xdg(|| {
            let log = SessionLog::new("cs1", "app1").expect("create");
            log.append(
                SessionEntryKind::Stop,
                serde_json::json!({"reason": "done"}),
            )
            .expect("append");
            let entries = SessionLog::read(&log.id).expect("read");
            assert_eq!(entries.len(), 2);
            assert!(matches!(entries[1].kind, SessionEntryKind::Stop));
        });
    }

    #[test]
    fn test_session_log_read_returns_empty_vec_for_nonexistent_session() {
        with_temp_xdg(|| {
            let entries = SessionLog::read("nonexistent-uuid-1234").expect("read");
            assert!(entries.is_empty());
        });
    }

    #[test]
    fn test_session_log_list_recent_returns_sessions_sorted_by_modified_time() {
        with_temp_xdg(|| {
            // Create three sessions with deliberate time gaps.
            let log1 = SessionLog::new("cs1", "app1").expect("create 1");
            // Touch the file to update mtime.
            std::thread::sleep(std::time::Duration::from_millis(20));
            let log2 = SessionLog::new("cs2", "app2").expect("create 2");
            std::thread::sleep(std::time::Duration::from_millis(20));
            let log3 = SessionLog::new("cs3", "app3").expect("create 3");

            let recent = SessionLog::list_recent(10).expect("list");
            assert_eq!(recent.len(), 3, "should list all three sessions");
            // Most recent first.
            assert_eq!(recent[0].0, log3.id, "most recent should be first");
            assert_eq!(recent[1].0, log2.id);
            assert_eq!(recent[2].0, log1.id);
            // mtime should be monotonic descending.
            assert!(recent[0].1 >= recent[1].1);
            assert!(recent[1].1 >= recent[2].1);
        });
    }

    #[test]
    fn test_session_log_list_recent_limits_to_n_entries() {
        with_temp_xdg(|| {
            let mut ids = Vec::new();
            for i in 0..5 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                let log = SessionLog::new(&format!("cs{}", i), "app").expect("create");
                ids.push(log.id);
            }
            let recent = SessionLog::list_recent(3).expect("list");
            assert_eq!(recent.len(), 3, "should limit to 3 entries");
            // Should be the 3 most recent — last 3 of the created sessions.
            assert_eq!(recent[0].0, ids[4]);
            assert_eq!(recent[1].0, ids[3]);
            assert_eq!(recent[2].0, ids[2]);
        });
    }

    #[test]
    fn test_session_log_list_recent_returns_empty_when_no_sessions() {
        with_temp_xdg(|| {
            let recent = SessionLog::list_recent(10).expect("list");
            assert!(recent.is_empty());
        });
    }

    #[test]
    fn test_session_log_path_returns_ndjson_file_path() {
        with_temp_xdg(|| {
            let log = SessionLog::new("cs", "app").expect("create");
            let path = log.path();
            let last = path
                .file_name()
                .and_then(|s| s.to_str())
                .expect("file name");
            assert!(
                last.ends_with(".ndjson"),
                "session file should end in .ndjson, got: {}",
                last
            );
        });
    }

    #[test]
    fn test_session_log_id_returns_uuid_v4_string() {
        with_temp_xdg(|| {
            let log = SessionLog::new("cs", "app").expect("create");
            let id = log.id();
            // UUID v4 string is 36 chars: 8-4-4-4-12 hex digits with hyphens.
            assert_eq!(
                id.len(),
                36,
                "UUID should be 36 chars, got: {} ({})",
                id.len(),
                id
            );
            let parts: Vec<&str> = id.split('-').collect();
            assert_eq!(
                parts.len(),
                5,
                "UUID should have 5 parts separated by hyphens"
            );
            assert_eq!(parts[0].len(), 8);
            assert_eq!(parts[1].len(), 4);
            assert_eq!(parts[2].len(), 4);
            assert_eq!(parts[3].len(), 4);
            assert_eq!(parts[4].len(), 12);
            // UUID v4: the third group should start with '4'.
            assert!(
                parts[2].starts_with('4'),
                "UUID v4 should have '4' as the first char of the third group, got: {}",
                parts[2]
            );
            // The fourth group should start with 8, 9, a, or b.
            let v = parts[3].chars().next().expect("first char");
            assert!(
                matches!(v, '8' | '9' | 'a' | 'b'),
                "UUID v4 variant should be 8/9/a/b, got: {}",
                v
            );
        });
    }

    #[test]
    fn test_session_entries_are_valid_ndjson() {
        with_temp_xdg(|| {
            let log = SessionLog::new("cs", "app").expect("create");
            log.append(
                SessionEntryKind::ExecStart,
                serde_json::json!({"cmd": "ls"}),
            )
            .expect("append");
            log.append(
                SessionEntryKind::ExecOutput,
                serde_json::json!({"stdout": "hello"}),
            )
            .expect("append");
            log.append(SessionEntryKind::ExecEnd, serde_json::json!({"exit": 0}))
                .expect("append");

            let content = std::fs::read_to_string(&log.path).expect("read");
            // Each line should parse as JSON.
            let mut count = 0;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let _: serde_json::Value =
                    serde_json::from_str(line).expect("each line should be valid JSON");
                count += 1;
            }
            assert_eq!(count, 4, "1 Connect + 3 appends = 4 NDJSON lines");
        });
    }

    #[test]
    fn test_session_entry_kinds_serialize_to_snake_case() {
        let cases = vec![
            (SessionEntryKind::Connect, "connect"),
            (SessionEntryKind::ExecStart, "exec_start"),
            (SessionEntryKind::ExecOutput, "exec_output"),
            (SessionEntryKind::ExecEnd, "exec_end"),
            (SessionEntryKind::HealthCheck, "health_check"),
            (SessionEntryKind::Hook, "hook"),
            (SessionEntryKind::Stop, "stop"),
            (SessionEntryKind::Warning, "warning"),
            (SessionEntryKind::Error, "error"),
        ];
        for (kind, expected) in cases {
            let entry = SessionEntry {
                timestamp: "2024-01-01T00:00:00Z".into(),
                kind,
                data: serde_json::Value::Null,
            };
            let json = serde_json::to_string(&entry).expect("serialize");
            assert!(
                json.contains(&format!("\"kind\":\"{}\"", expected)),
                "expected kind to serialize as '{}', got: {}",
                expected,
                json
            );
        }
    }

    #[test]
    fn test_session_entry_kinds_deserialize_from_snake_case() {
        let cases = vec![
            ("connect", SessionEntryKind::Connect),
            ("exec_start", SessionEntryKind::ExecStart),
            ("exec_output", SessionEntryKind::ExecOutput),
            ("exec_end", SessionEntryKind::ExecEnd),
            ("health_check", SessionEntryKind::HealthCheck),
            ("hook", SessionEntryKind::Hook),
            ("stop", SessionEntryKind::Stop),
            ("warning", SessionEntryKind::Warning),
            ("error", SessionEntryKind::Error),
        ];
        for (snake, expected) in cases {
            let json = format!(
                r#"{{"timestamp":"2024-01-01T00:00:00Z","kind":"{}","data":null}}"#,
                snake
            );
            let entry: SessionEntry = serde_json::from_str(&json).expect("deserialize");
            // Compare via debug format since SessionEntryKind doesn't impl PartialEq.
            assert_eq!(format!("{:?}", entry.kind), format!("{:?}", expected));
        }
    }

    #[test]
    fn test_session_entry_does_not_serialize_pascal_case() {
        let entry = SessionEntry {
            timestamp: "2024".into(),
            kind: SessionEntryKind::ExecStart,
            data: serde_json::Value::Null,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        // Must NOT contain the PascalCase variant name.
        assert!(
            !json.contains("ExecStart"),
            "session kind should NOT serialize as PascalCase, got: {}",
            json
        );
    }

    #[test]
    fn test_session_log_append_creates_file_if_missing() {
        with_temp_xdg(|| {
            // Construct a SessionLog struct without calling new() so the
            // file doesn't exist yet. Create the parent dir first (as new()
            // would). Then append() should create the file itself.
            let id = uuid::Uuid::new_v4().to_string();
            let path = sessions_dir().join(format!("{}.ndjson", id));
            std::fs::create_dir_all(sessions_dir()).expect("create sessions dir");
            let log = SessionLog {
                id: id.clone(),
                path: path.clone(),
            };
            assert!(!path.exists());
            log.append(SessionEntryKind::Stop, serde_json::json!({"x": 1}))
                .expect("append should create file");
            assert!(path.exists());
            let entries = SessionLog::read(&id).expect("read");
            assert_eq!(entries.len(), 1);
            assert!(matches!(entries[0].kind, SessionEntryKind::Stop));
        });
    }

    #[test]
    fn test_session_log_read_skips_blank_lines() {
        with_temp_xdg(|| {
            let log = SessionLog::new("cs", "app").expect("create");
            // Append some blank lines manually.
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log.path)
                .expect("open");
            writeln!(f).expect("blank line");
            writeln!(f, "   ").expect("whitespace-only line");
            writeln!(f, "").expect("another blank line");
            drop(f);

            log.append(SessionEntryKind::Warning, serde_json::json!({"m": "test"}))
                .expect("append");

            let entries = SessionLog::read(&log.id).expect("read");
            // Connect + Warning = 2 (blank lines should be skipped).
            assert_eq!(entries.len(), 2);
            assert!(matches!(entries[1].kind, SessionEntryKind::Warning));
        });
    }

    #[test]
    fn test_session_log_list_recent_skips_non_ndjson_files() {
        with_temp_xdg(|| {
            // Create a real session.
            let _log = SessionLog::new("cs", "app").expect("create");
            // Drop a random non-ndjson file in the sessions dir.
            std::fs::create_dir_all(sessions_dir()).unwrap();
            std::fs::write(sessions_dir().join("README.txt"), "hi").unwrap();
            std::fs::write(sessions_dir().join("notes.json"), "{}").unwrap();
            let recent = SessionLog::list_recent(10).expect("list");
            // Only the .ndjson session should be counted.
            assert_eq!(recent.len(), 1);
        });
    }

    #[test]
    fn test_session_log_list_recent_zero_returns_empty() {
        with_temp_xdg(|| {
            let _log = SessionLog::new("cs", "app").expect("create");
            let recent = SessionLog::list_recent(0).expect("list");
            assert!(recent.is_empty(), "list_recent(0) should return empty");
        });
    }

    #[test]
    fn test_session_log_new_creates_parent_dir() {
        with_temp_xdg(|| {
            // The sessions dir shouldn't exist yet (XDG points at empty temp).
            assert!(!sessions_dir().exists());
            let log = SessionLog::new("cs", "app").expect("create");
            assert!(sessions_dir().exists(), "sessions dir should be created");
            assert!(log.path.exists());
        });
    }

    #[test]
    fn test_session_path_id_consistency() {
        with_temp_xdg(|| {
            let log = SessionLog::new("cs", "app").expect("create");
            // The file name (minus .ndjson) should match the id.
            let file_name = log
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("file stem");
            assert_eq!(file_name, log.id);
        });
    }

    #[test]
    fn test_session_entry_timestamp_is_rfc3339() {
        with_temp_xdg(|| {
            let log = SessionLog::new("cs", "app").expect("create");
            let entries = SessionLog::read(&log.id).expect("read");
            let ts = &entries[0].timestamp;
            // RFC3339 timestamps contain 'T' (date-time separator) and 'Z' (UTC).
            assert!(
                ts.contains('T'),
                "timestamp should be RFC3339 with T, got: {}",
                ts
            );
            assert!(
                ts.ends_with('Z') || ts.contains('+'),
                "timestamp should end with Z or +offset, got: {}",
                ts
            );
            // Should be parseable as a chrono DateTime.
            let _dt: chrono::DateTime<chrono::Utc> = chrono::DateTime::parse_from_rfc3339(ts)
                .expect("timestamp should parse as RFC3339")
                .with_timezone(&chrono::Utc);
        });
    }
}
