//! Per-codespace state entry.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodespaceState {
    /// "Available", "Shutdown", "Starting", "ShuttingDown", etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_known_state: Option<String>,

    /// ISO 8601 timestamp of last state check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<String>,

    /// Codespace creation time (from GitHub API). Used for host-key rotation detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// SSH host key fingerprint (e.g., "SHA256:abc...").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_key_fingerprint: Option<String>,

    /// When the host key was first stored (for rotation detection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_key_stored_at: Option<String>,

    /// Last health check result: "green" or "red" (None = never checked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_health_status: Option<String>,

    /// ISO 8601 timestamp of last health check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_health_checked_at: Option<String>,
}

impl CodespaceState {
    /// Check if the host key should be considered stale (codespace was rebuilt).
    /// Returns true if `created_at` is newer than `host_key_stored_at`.
    pub fn host_key_is_stale(&self) -> bool {
        match (&self.created_at, &self.host_key_stored_at) {
            (Some(created), Some(stored)) => created > stored,
            _ => false,
        }
    }
}
