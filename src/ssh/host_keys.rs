//! Host key store: TOFU with rotation detection.

use crate::state::CodespaceState;
use crate::{CodespaceError, Result};

/// Compute SHA-256 fingerprint of an SSH host key.
pub fn fingerprint(host_key: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(host_key);
    let digest = hasher.finalize();
    format!("SHA256:{}", base64_encode(&digest))
}

/// Base64 encode (standard alphabet, with padding).
fn base64_encode(bytes: &[u8]) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let combined = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARSET[((combined >> 18) & 0x3F) as usize] as char);
        result.push(CHARSET[((combined >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARSET[((combined >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARSET[(combined & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Errors from host key operations.
#[derive(Debug, thiserror::Error)]
pub enum HostKeyError {
    #[error("host key mismatch: expected {expected}, got {actual}")]
    Mismatch { expected: String, actual: String },

    #[error("host key not yet stored (first connect)")]
    NotStored,

    #[error("failed to compute fingerprint: {0}")]
    FingerprintFailed(String),
}

/// Decide what to do given a host key and the stored codespace state.
pub enum HostKeyDecision {
    /// First connect — store the key.
    StoreNew,
    /// Key matches stored value — proceed.
    Match,
    /// Codespace was rebuilt since key was stored — rotate, accept new key.
    Rotate { old: String, new: String },
    /// Key mismatch with no explanation — fail (possible MITM).
    Reject { expected: String, actual: String },
}

/// Decide based on the incoming host key and the stored state.
pub fn decide(incoming_fp: &str, state: &CodespaceState, accept_new: bool) -> HostKeyDecision {
    match &state.host_key_fingerprint {
        None => {
            if accept_new {
                HostKeyDecision::StoreNew
            } else {
                HostKeyDecision::StoreNew // first connect always stores
            }
        }
        Some(stored) if stored == incoming_fp => HostKeyDecision::Match,
        Some(stored) => {
            // Mismatch — check if codespace was rebuilt
            if state.host_key_is_stale() {
                HostKeyDecision::Rotate {
                    old: stored.clone(),
                    new: incoming_fp.to_string(),
                }
            } else {
                HostKeyDecision::Reject {
                    expected: stored.clone(),
                    actual: incoming_fp.to_string(),
                }
            }
        }
    }
}

/// Convert a decision into either Ok or Err.
pub fn enforce_decision(
    decision: HostKeyDecision,
    accept_rotation: bool,
) -> Result<Option<String>> {
    match decision {
        HostKeyDecision::StoreNew => Ok(None), // signal "store this key"
        HostKeyDecision::Match => Ok(Some("match".into())),
        HostKeyDecision::Rotate { old, new } => {
            if accept_rotation {
                Ok(Some(format!("rotated: {} -> {}", old, new)))
            } else {
                Err(CodespaceError::HostKeyMismatch {
                    expected: old,
                    actual: new,
                })
            }
        }
        HostKeyDecision::Reject { expected, actual } => {
            Err(CodespaceError::HostKeyMismatch { expected, actual })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_deterministic() {
        let fp1 = fingerprint(b"hello world");
        let fp2 = fingerprint(b"hello world");
        assert_eq!(fp1, fp2);
        assert!(fp1.starts_with("SHA256:"));
    }

    #[test]
    fn test_fingerprint_differs_for_different_keys() {
        let fp1 = fingerprint(b"key1");
        let fp2 = fingerprint(b"key2");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_base64_encode_basic() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"hi"), "aGk=");
        assert_eq!(base64_encode(b"x"), "eA==");
    }

    #[test]
    fn test_decide_store_new_first_connect() {
        let state = CodespaceState::default();
        let decision = decide("SHA256:abc", &state, false);
        assert!(matches!(decision, HostKeyDecision::StoreNew));
    }

    #[test]
    fn test_decide_match() {
        let mut state = CodespaceState::default();
        state.host_key_fingerprint = Some("SHA256:abc".into());
        let decision = decide("SHA256:abc", &state, false);
        assert!(matches!(decision, HostKeyDecision::Match));
    }

    #[test]
    fn test_decide_reject_on_mismatch() {
        let mut state = CodespaceState::default();
        state.host_key_fingerprint = Some("SHA256:abc".into());
        // created_at NOT newer than host_key_stored_at → not stale → reject
        state.created_at = Some("2026-01-01T00:00:00Z".into());
        state.host_key_stored_at = Some("2026-01-02T00:00:00Z".into());
        let decision = decide("SHA256:xyz", &state, false);
        assert!(matches!(decision, HostKeyDecision::Reject { .. }));
    }

    #[test]
    fn test_decide_rotate_on_rebuild() {
        let mut state = CodespaceState::default();
        state.host_key_fingerprint = Some("SHA256:abc".into());
        // created_at IS newer than host_key_stored_at → stale → rotate
        state.created_at = Some("2026-02-01T00:00:00Z".into());
        state.host_key_stored_at = Some("2026-01-01T00:00:00Z".into());
        let decision = decide("SHA256:xyz", &state, false);
        assert!(matches!(decision, HostKeyDecision::Rotate { .. }));
    }
}

/// Host key store interface — wraps the per-codespace state.
pub struct HostKeyStore;

impl HostKeyStore {
    /// Get the stored fingerprint for a codespace, or None if first connect.
    pub fn get(codespace_name: &str) -> Result<Option<String>> {
        let state = crate::state::load_state()?;
        Ok(state
            .codespaces
            .get(codespace_name)
            .and_then(|c| c.host_key_fingerprint.clone()))
    }

    /// Store a new fingerprint for a codespace.
    pub fn store(codespace_name: &str, fingerprint: &str) -> Result<()> {
        let mut state = crate::state::load_state()?;
        let entry = state
            .codespaces
            .entry(codespace_name.to_string())
            .or_default();
        entry.host_key_fingerprint = Some(fingerprint.to_string());
        entry.host_key_stored_at = Some(chrono::Utc::now().to_rfc3339());
        crate::state::save_state(&state)
    }

    /// Remove the stored fingerprint for a codespace.
    pub fn clear(codespace_name: &str) -> Result<()> {
        let mut state = crate::state::load_state()?;
        if let Some(entry) = state.codespaces.get_mut(codespace_name) {
            entry.host_key_fingerprint = None;
            entry.host_key_stored_at = None;
        }
        crate::state::save_state(&state)
    }
}
