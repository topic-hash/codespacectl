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
