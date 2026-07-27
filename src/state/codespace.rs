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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_has_all_none_fields() {
        let cs = CodespaceState::default();
        assert!(cs.last_known_state.is_none());
        assert!(cs.last_checked_at.is_none());
        assert!(cs.created_at.is_none());
        assert!(cs.host_key_fingerprint.is_none());
        assert!(cs.host_key_stored_at.is_none());
        assert!(cs.last_health_status.is_none());
        assert!(cs.last_health_checked_at.is_none());
    }

    #[test]
    fn test_host_key_is_stale_false_when_both_none() {
        let cs = CodespaceState::default();
        assert!(!cs.host_key_is_stale());
    }

    #[test]
    fn test_host_key_is_stale_false_when_created_at_none() {
        let cs = CodespaceState {
            host_key_stored_at: Some("2024-01-01T00:00:00Z".into()),
            ..Default::default()
        };
        assert!(!cs.host_key_is_stale());
    }

    #[test]
    fn test_host_key_is_stale_false_when_host_key_stored_at_none() {
        let cs = CodespaceState {
            created_at: Some("2024-01-01T00:00:00Z".into()),
            ..Default::default()
        };
        assert!(!cs.host_key_is_stale());
    }

    #[test]
    fn test_host_key_is_stale_false_when_created_equals_stored() {
        let ts = "2024-01-01T00:00:00Z";
        let cs = CodespaceState {
            created_at: Some(ts.into()),
            host_key_stored_at: Some(ts.into()),
            ..Default::default()
        };
        assert!(!cs.host_key_is_stale());
    }

    #[test]
    fn test_host_key_is_stale_false_when_created_older_than_stored() {
        // Codespace was created BEFORE the host key was stored (older codespace).
        // host_key_is_stale should return false (codespace wasn't rebuilt).
        let cs = CodespaceState {
            created_at: Some("2024-01-01T00:00:00Z".into()),
            host_key_stored_at: Some("2024-06-01T00:00:00Z".into()),
            ..Default::default()
        };
        assert!(!cs.host_key_is_stale());
    }

    #[test]
    fn test_host_key_is_stale_true_when_created_newer_than_stored() {
        // Codespace was created AFTER the host key was stored (codespace rebuilt).
        // host_key_is_stale should return true.
        let cs = CodespaceState {
            created_at: Some("2024-06-01T00:00:00Z".into()),
            host_key_stored_at: Some("2024-01-01T00:00:00Z".into()),
            ..Default::default()
        };
        assert!(cs.host_key_is_stale());
    }

    #[test]
    fn test_host_key_is_stale_uses_lexicographic_comparison() {
        // Same date, different time — string comparison should give correct order.
        let cs = CodespaceState {
            created_at: Some("2024-01-01T12:00:00Z".into()),
            host_key_stored_at: Some("2024-01-01T00:00:00Z".into()),
            ..Default::default()
        };
        assert!(cs.host_key_is_stale());
    }

    #[test]
    fn test_codespace_state_serializes_to_json() {
        let cs = CodespaceState {
            last_known_state: Some("Available".into()),
            last_checked_at: Some("2024-01-01T00:00:00Z".into()),
            created_at: Some("2024-01-01T00:00:00Z".into()),
            host_key_fingerprint: Some("SHA256:abc".into()),
            host_key_stored_at: Some("2024-01-01T00:00:00Z".into()),
            last_health_status: Some("green".into()),
            last_health_checked_at: Some("2024-01-01T00:00:00Z".into()),
        };
        let json = serde_json::to_string(&cs).expect("serialize");
        // Should contain all the values.
        assert!(json.contains("Available"));
        assert!(json.contains("SHA256:abc"));
        assert!(json.contains("green"));
        // None fields should be skipped via skip_serializing_if.
        assert!(!json.contains("null"));
    }

    #[test]
    fn test_codespace_state_serializes_empty_to_empty_object() {
        let cs = CodespaceState::default();
        let json = serde_json::to_string(&cs).expect("serialize");
        // All fields are None and skipped — should be an empty JSON object.
        assert_eq!(json, "{}");
    }

    #[test]
    fn test_codespace_state_deserializes_from_json() {
        let json = r#"{
            "last_known_state": "Available",
            "last_checked_at": "2024-01-01T00:00:00Z",
            "created_at": "2024-01-01T00:00:00Z",
            "host_key_fingerprint": "SHA256:abc",
            "host_key_stored_at": "2024-01-01T00:00:00Z",
            "last_health_status": "green",
            "last_health_checked_at": "2024-01-01T00:00:00Z"
        }"#;
        let cs: CodespaceState = serde_json::from_str(json).expect("deserialize");
        assert_eq!(cs.last_known_state.as_deref(), Some("Available"));
        assert_eq!(cs.last_checked_at.as_deref(), Some("2024-01-01T00:00:00Z"));
        assert_eq!(cs.created_at.as_deref(), Some("2024-01-01T00:00:00Z"));
        assert_eq!(cs.host_key_fingerprint.as_deref(), Some("SHA256:abc"));
        assert_eq!(
            cs.host_key_stored_at.as_deref(),
            Some("2024-01-01T00:00:00Z")
        );
        assert_eq!(cs.last_health_status.as_deref(), Some("green"));
        assert_eq!(
            cs.last_health_checked_at.as_deref(),
            Some("2024-01-01T00:00:00Z")
        );
    }

    #[test]
    fn test_codespace_state_round_trip_all_fields() {
        let original = CodespaceState {
            last_known_state: Some("Shutdown".into()),
            last_checked_at: Some("2024-12-31T23:59:59Z".into()),
            created_at: Some("2024-06-15T10:30:00Z".into()),
            host_key_fingerprint: Some("SHA256:deadbeef".into()),
            host_key_stored_at: Some("2024-06-15T10:31:00Z".into()),
            last_health_status: Some("red".into()),
            last_health_checked_at: Some("2024-06-15T11:00:00Z".into()),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CodespaceState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.last_known_state, original.last_known_state);
        assert_eq!(back.last_checked_at, original.last_checked_at);
        assert_eq!(back.created_at, original.created_at);
        assert_eq!(back.host_key_fingerprint, original.host_key_fingerprint);
        assert_eq!(back.host_key_stored_at, original.host_key_stored_at);
        assert_eq!(back.last_health_status, original.last_health_status);
        assert_eq!(back.last_health_checked_at, original.last_health_checked_at);
    }

    #[test]
    fn test_codespace_state_round_trip_default() {
        let original = CodespaceState::default();
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CodespaceState = serde_json::from_str(&json).expect("deserialize");
        // All fields None.
        assert!(back.last_known_state.is_none());
        assert!(back.last_checked_at.is_none());
        assert!(back.created_at.is_none());
        assert!(back.host_key_fingerprint.is_none());
        assert!(back.host_key_stored_at.is_none());
        assert!(back.last_health_status.is_none());
        assert!(back.last_health_checked_at.is_none());
    }

    #[test]
    fn test_codespace_state_deserializes_partial_json() {
        // Missing fields should default to None.
        let json = r#"{"last_known_state": "Starting"}"#;
        let cs: CodespaceState = serde_json::from_str(json).expect("deserialize");
        assert_eq!(cs.last_known_state.as_deref(), Some("Starting"));
        assert!(cs.last_checked_at.is_none());
        assert!(cs.created_at.is_none());
        assert!(cs.host_key_fingerprint.is_none());
        assert!(cs.host_key_stored_at.is_none());
        assert!(cs.last_health_status.is_none());
        assert!(cs.last_health_checked_at.is_none());
    }

    #[test]
    fn test_codespace_state_deserializes_empty_object() {
        let json = "{}";
        let cs: CodespaceState = serde_json::from_str(json).expect("deserialize");
        // All fields None — matches Default.
        assert!(cs.last_known_state.is_none());
        assert!(cs.created_at.is_none());
        assert!(cs.host_key_stored_at.is_none());
    }

    #[test]
    fn test_codespace_state_clone_preserves_fields() {
        let cs = CodespaceState {
            last_known_state: Some("Available".into()),
            created_at: Some("2024-01-01T00:00:00Z".into()),
            host_key_fingerprint: Some("SHA256:abc".into()),
            ..Default::default()
        };
        let cloned = cs.clone();
        assert_eq!(cloned.last_known_state, cs.last_known_state);
        assert_eq!(cloned.created_at, cs.created_at);
        assert_eq!(cloned.host_key_fingerprint, cs.host_key_fingerprint);
    }

    #[test]
    fn test_codespace_state_debug_format_contains_fields() {
        let cs = CodespaceState {
            last_known_state: Some("Available".into()),
            ..Default::default()
        };
        let s = format!("{:?}", cs);
        assert!(s.contains("CodespaceState"));
        assert!(s.contains("Available"));
    }
}
