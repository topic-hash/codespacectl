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
    fn test_fingerprint_of_empty_slice_is_valid() {
        // SHA256 of empty input is a well-known constant; we don't assert on
        // the exact value, but we do require the format to be `SHA256:<base64>`
        // with a non-empty base64 segment.
        let fp = fingerprint(b"");
        assert!(
            fp.starts_with("SHA256:"),
            "fingerprint should start with 'SHA256:', got {}",
            fp
        );
        let b64 = &fp["SHA256:".len()..];
        assert!(!b64.is_empty(), "base64 portion should not be empty");
        // SHA256 produces a 32-byte digest → 44 base64 chars (with padding).
        assert_eq!(b64.len(), 44, "expected 44-char base64, got {}", b64);
    }

    #[test]
    fn test_fingerprint_of_32_byte_ed25519_key_is_reasonable() {
        // A 32-byte Ed25519 public key (the raw scalar) — make sure the
        // fingerprint is well-formed (SHA256: prefix + non-empty base64).
        let key: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ];
        let fp = fingerprint(&key);
        assert!(fp.starts_with("SHA256:"));
        let b64 = &fp["SHA256:".len()..];
        assert_eq!(b64.len(), 44, "expected 44-char base64 for 32-byte key");
    }

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn test_base64_encode_one_byte() {
        // 1 byte → 2 base64 chars + 2 '=' padding
        assert_eq!(base64_encode(b"x"), "eA==");
        assert_eq!(base64_encode(b"\0"), "AA==");
    }

    #[test]
    fn test_base64_encode_two_bytes() {
        // 2 bytes → 3 base64 chars + 1 '=' padding
        assert_eq!(base64_encode(b"hi"), "aGk=");
        assert_eq!(base64_encode(b"ab"), "YWI=");
    }

    #[test]
    fn test_base64_encode_three_bytes() {
        // 3 bytes → 4 base64 chars, no padding
        assert_eq!(base64_encode(b"abc"), "YWJj");
        assert_eq!(base64_encode(b"xyz"), "eHl6");
    }

    #[test]
    fn test_base64_encode_four_bytes() {
        // 4 bytes → "abcd" → 6 chars + 1 '=' padding
        assert_eq!(base64_encode(b"abcd"), "YWJjZA==");
    }

    #[test]
    fn test_base64_encode_known_value_hello() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn test_base64_encode_known_value_hi() {
        assert_eq!(base64_encode(b"hi"), "aGk=");
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

    #[test]
    fn test_decide_accept_new_true_still_store_new_for_first_connect() {
        // Per the spec: accept_new=true still returns StoreNew on first
        // connect (same as default).
        let state = CodespaceState::default();
        let decision = decide("SHA256:abc", &state, true);
        assert!(matches!(decision, HostKeyDecision::StoreNew));
    }

    #[test]
    fn test_enforce_decision_store_new_returns_ok_none() {
        let r = enforce_decision(HostKeyDecision::StoreNew, false).unwrap();
        assert!(r.is_none(), "StoreNew should return Ok(None)");
    }

    #[test]
    fn test_enforce_decision_match_returns_ok_some_match() {
        let r = enforce_decision(HostKeyDecision::Match, false).unwrap();
        assert!(r.is_some(), "Match should return Ok(Some)");
        assert_eq!(r.unwrap(), "match");
    }

    #[test]
    fn test_enforce_decision_rotate_with_accept_returns_ok_some() {
        let decision = HostKeyDecision::Rotate {
            old: "SHA256:abc".into(),
            new: "SHA256:xyz".into(),
        };
        let r = enforce_decision(decision, true).expect("Rotate + accept should succeed");
        assert!(r.is_some());
        let s = r.unwrap();
        assert!(s.contains("rotated"), "expected 'rotated' in {:?}", s);
        assert!(s.contains("SHA256:abc"), "expected old key in {:?}", s);
        assert!(s.contains("SHA256:xyz"), "expected new key in {:?}", s);
    }

    #[test]
    fn test_enforce_decision_rotate_without_accept_returns_err() {
        let decision = HostKeyDecision::Rotate {
            old: "SHA256:abc".into(),
            new: "SHA256:xyz".into(),
        };
        let err = enforce_decision(decision, false).unwrap_err();
        assert!(
            matches!(err, CodespaceError::HostKeyMismatch { .. }),
            "Rotate + reject should map to HostKeyMismatch, got {:?}",
            err
        );
    }

    #[test]
    fn test_enforce_decision_reject_returns_err_regardless_of_accept() {
        fn make_decision() -> HostKeyDecision {
            HostKeyDecision::Reject {
                expected: "SHA256:abc".into(),
                actual: "SHA256:xyz".into(),
            }
        }
        // Even with accept_rotation=true, Reject must return Err.
        let err = enforce_decision(make_decision(), true).unwrap_err();
        assert!(matches!(err, CodespaceError::HostKeyMismatch { .. }));
        // Same with accept_rotation=false.
        let err = enforce_decision(make_decision(), false).unwrap_err();
        assert!(matches!(err, CodespaceError::HostKeyMismatch { .. }));
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    /// Serialize state-file-touching tests so they don't race each other on
    /// `XDG_CACHE_HOME` (which controls `state_dir()`).
    static STATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn state_lock() -> &'static Mutex<()> {
        STATE_LOCK.get_or_init(|| Mutex::new(()))
    }

    /// RAII guard: redirect XDG_CACHE_HOME into a fresh tempdir for the
    /// duration of the test, and restore the previous value on drop.
    struct CacheDirGuard {
        prev: Option<std::ffi::OsString>,
    }
    impl CacheDirGuard {
        fn new() -> Self {
            let prev = std::env::var_os("XDG_CACHE_HOME");
            let tmp = tempfile::tempdir().expect("tempdir");
            std::env::set_var("XDG_CACHE_HOME", tmp.path());
            // Hold the tempdir for the life of the guard by leaking — the
            // OS cleans up the tempdir at process exit anyway.
            std::mem::forget(tmp);
            Self { prev }
        }
    }
    impl Drop for CacheDirGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
                None => std::env::remove_var("XDG_CACHE_HOME"),
            }
        }
    }

    #[test]
    fn test_host_key_store_get_returns_none_for_unknown_codespace() {
        let _g = state_lock().lock().unwrap();
        let _cache = CacheDirGuard::new();
        let r = HostKeyStore::get("never-heard-of-this").expect("get should not error");
        assert!(r.is_none(), "unknown codespace should have no stored key");
    }

    #[test]
    fn test_host_key_store_store_get_round_trip() {
        let _g = state_lock().lock().unwrap();
        let _cache = CacheDirGuard::new();
        let fp = "SHA256:round_trip_test_value";

        HostKeyStore::store("cs-rt", fp).expect("store should succeed");
        let got = HostKeyStore::get("cs-rt").expect("get should succeed");
        assert_eq!(got.as_deref(), Some(fp));
    }

    #[test]
    fn test_host_key_store_clear_removes_fingerprint() {
        let _g = state_lock().lock().unwrap();
        let _cache = CacheDirGuard::new();
        HostKeyStore::store("cs-clear", "SHA256:abc").expect("store should succeed");
        assert_eq!(
            HostKeyStore::get("cs-clear").unwrap().as_deref(),
            Some("SHA256:abc")
        );
        HostKeyStore::clear("cs-clear").expect("clear should succeed");
        assert!(
            HostKeyStore::get("cs-clear").unwrap().is_none(),
            "after clear, get should return None"
        );
    }

    #[test]
    fn test_host_key_store_clear_on_unknown_codespace_does_not_error() {
        let _g = state_lock().lock().unwrap();
        let _cache = CacheDirGuard::new();
        // Clearing a codespace that was never stored must return Ok.
        HostKeyStore::clear("never-stored").expect("clear on unknown codespace should return Ok");
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
