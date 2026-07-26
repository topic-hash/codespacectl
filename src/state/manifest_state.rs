//! Per-manifest state entry.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestState {
    /// SHA-256 of the manifest content (for change detection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,

    /// ISO 8601 timestamp of last validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_validated_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_has_all_none_fields() {
        let ms = ManifestState::default();
        assert!(ms.sha256.is_none());
        assert!(ms.last_validated_at.is_none());
    }

    #[test]
    fn test_serialization_with_all_fields() {
        let ms = ManifestState {
            sha256: Some("abc123".into()),
            last_validated_at: Some("2024-01-01T00:00:00Z".into()),
        };
        let json = serde_json::to_string(&ms).expect("serialize");
        assert!(json.contains("abc123"));
        assert!(json.contains("2024-01-01T00:00:00Z"));
        // No None fields should appear.
        assert!(!json.contains("null"));
    }

    #[test]
    fn test_serialization_with_only_sha256() {
        let ms = ManifestState {
            sha256: Some("abc".into()),
            last_validated_at: None,
        };
        let json = serde_json::to_string(&ms).expect("serialize");
        assert!(json.contains("abc"));
        assert!(!json.contains("null"));
        assert!(!json.contains("last_validated_at"));
    }

    #[test]
    fn test_serialization_with_only_last_validated_at() {
        let ms = ManifestState {
            sha256: None,
            last_validated_at: Some("2024".into()),
        };
        let json = serde_json::to_string(&ms).expect("serialize");
        assert!(json.contains("2024"));
        assert!(!json.contains("null"));
        assert!(!json.contains("sha256"));
    }

    #[test]
    fn test_serialization_default_is_empty_object() {
        let ms = ManifestState::default();
        let json = serde_json::to_string(&ms).expect("serialize");
        // All fields None and skipped — empty JSON object.
        assert_eq!(json, "{}");
    }

    #[test]
    fn test_round_trip_all_fields() {
        let original = ManifestState {
            sha256: Some("deadbeef".into()),
            last_validated_at: Some("2024-06-15T10:00:00Z".into()),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: ManifestState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.sha256, original.sha256);
        assert_eq!(back.last_validated_at, original.last_validated_at);
    }

    #[test]
    fn test_round_trip_default() {
        let original = ManifestState::default();
        let json = serde_json::to_string(&original).expect("serialize");
        let back: ManifestState = serde_json::from_str(&json).expect("deserialize");
        assert!(back.sha256.is_none());
        assert!(back.last_validated_at.is_none());
    }

    #[test]
    fn test_partial_json_only_sha256_deserializes_with_none_for_missing_field() {
        // Only sha256 present, last_validated_at absent.
        let json = r#"{"sha256": "abc123"}"#;
        let ms: ManifestState = serde_json::from_str(json).expect("deserialize");
        assert_eq!(ms.sha256.as_deref(), Some("abc123"));
        assert!(ms.last_validated_at.is_none());
    }

    #[test]
    fn test_partial_json_only_last_validated_at_deserializes() {
        // Only last_validated_at present, sha256 absent.
        let json = r#"{"last_validated_at": "2024-01-01T00:00:00Z"}"#;
        let ms: ManifestState = serde_json::from_str(json).expect("deserialize");
        assert!(ms.sha256.is_none());
        assert_eq!(ms.last_validated_at.as_deref(), Some("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn test_empty_json_object_deserializes_to_default() {
        let json = "{}";
        let ms: ManifestState = serde_json::from_str(json).expect("deserialize");
        assert!(ms.sha256.is_none());
        assert!(ms.last_validated_at.is_none());
    }

    #[test]
    fn test_clone_preserves_fields() {
        let ms = ManifestState {
            sha256: Some("abc".into()),
            last_validated_at: Some("2024".into()),
        };
        let cloned = ms.clone();
        assert_eq!(cloned.sha256, ms.sha256);
        assert_eq!(cloned.last_validated_at, ms.last_validated_at);
    }

    #[test]
    fn test_debug_format() {
        let ms = ManifestState {
            sha256: Some("abc".into()),
            last_validated_at: None,
        };
        let s = format!("{:?}", ms);
        assert!(s.contains("ManifestState"));
        assert!(s.contains("abc"));
    }
}
