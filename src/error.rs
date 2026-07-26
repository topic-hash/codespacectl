//! Typed error model. Every failure produces a structured error with:
//! - `kind`: stable string identifier (closed set)
//! - `retryable`: bool — should the caller retry?
//! - `suggested_action`: human-readable next step
//! - `context`: optional structured context (codespace name, attempt count, etc.)
//!
//! All error variants map to one of:
//! - HTTP-style exit codes (0=ok, 65=config, 70=internal, 75=temp-fail, 76=protocol)
//! - JSON envelope `error.kind` field
//!
//! ## Error kind catalog (closed set, versioned)
//!
//! | kind                          | retryable | exit | When                                    |
//! |-------------------------------|-----------|------|-----------------------------------------|
//! | `binary_missing`              | false     | 70   | gh binary not found                     |
//! | `binary_hash_mismatch`        | false     | 70   | gh binary SHA-256 mismatch              |
//! | `auth_failed`                 | false     | 70   | 401 from GitHub API                     |
//! | `token_revoked`               | false     | 70   | 401 + token invalid                     |
//! | `token_invalid_scope`         | false     | 70   | 403 missing scope                       |
//! | `token_missing`               | false     | 65   | No env var or token file                |
//! | `codespace_not_found`         | false     | 70   | 404 from GitHub                        |
//! | `codespace_start_timeout`     | true      | 75   | Codespace didn't reach Available       |
//! | `codespace_unreachable`       | true      | 75   | Network error to GitHub                |
//! | `health_check_failed`         | false     | 70   | Manifest health check returned non-zero |
//! | `command_timeout`             | true      | 75   | exec exceeded timeout_secs             |
//! | `command_failed`              | false     | 70   | exec returned non-zero exit code        |
//! | `host_key_mismatch`           | false     | 76   | SSH host key changed unexpectedly       |
//! | `manifest_invalid`            | false     | 65   | CODESPACE.yaml schema violation         |
//! | `manifest_version_unsupported`| false     | 65   | apiVersion not v1                       |
//! | `manifest_not_found`          | false     | 65   | No CODESPACE.yaml at path               |
//! | `network_error`               | true      | 75   | Generic network failure                 |
//! | `internal_error`              | false     | 70   | Unexpected                              |

use serde::{Deserialize, Serialize};
use std::fmt;

pub type Result<T> = std::result::Result<T, CodespaceError>;

#[derive(Debug, thiserror::Error)]
pub enum CodespaceError {
    #[error("binary missing: {0}")]
    BinaryMissing(String),

    #[error("binary hash mismatch: expected {expected}, got {actual}")]
    BinaryHashMismatch { expected: String, actual: String },

    #[error("auth failed: {0}")]
    AuthFailed(String),

    #[error("token revoked")]
    TokenRevoked,

    #[error("token invalid scope: missing {scope}")]
    TokenInvalidScope { scope: String },

    #[error("token missing: set CODESPACECTL_TOKEN env var or run `codespacectl token set`")]
    TokenMissing,

    #[error("codespace not found: {0}")]
    CodespaceNotFound(String),

    #[error("codespace start timeout: {elapsed_secs}s elapsed")]
    CodespaceStartTimeout { elapsed_secs: u64 },

    #[error("codespace unreachable: {0}")]
    CodespaceUnreachable(String),

    #[error("health check failed: {check}")]
    HealthCheckFailed { check: String, exit_code: i32, stderr: String },

    #[error("command timeout: {timeout_secs}s")]
    CommandTimeout { timeout_secs: u64 },

    #[error("command failed: exit code {exit_code}")]
    CommandFailed { exit_code: i32, stderr: String },

    #[error("host key mismatch: expected {expected}, got {actual}")]
    HostKeyMismatch { expected: String, actual: String },

    #[error("manifest invalid: {0}")]
    ManifestInvalid(String),

    #[error("manifest version unsupported: {0}")]
    ManifestVersionUnsupported(String),

    #[error("manifest not found: {0}")]
    ManifestNotFound(String),

    #[error("network error: {0}")]
    NetworkError(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl CodespaceError {
    /// Stable string identifier for the error kind.
    /// Used in JSON envelope `error.kind` field.
    /// Closed set — see module docs for full catalog.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::BinaryMissing(_) => "binary_missing",
            Self::BinaryHashMismatch { .. } => "binary_hash_mismatch",
            Self::AuthFailed(_) => "auth_failed",
            Self::TokenRevoked => "token_revoked",
            Self::TokenInvalidScope { .. } => "token_invalid_scope",
            Self::TokenMissing => "token_missing",
            Self::CodespaceNotFound(_) => "codespace_not_found",
            Self::CodespaceStartTimeout { .. } => "codespace_start_timeout",
            Self::CodespaceUnreachable(_) => "codespace_unreachable",
            Self::HealthCheckFailed { .. } => "health_check_failed",
            Self::CommandTimeout { .. } => "command_timeout",
            Self::CommandFailed { .. } => "command_failed",
            Self::HostKeyMismatch { .. } => "host_key_mismatch",
            Self::ManifestInvalid(_) => "manifest_invalid",
            Self::ManifestVersionUnsupported(_) => "manifest_version_unsupported",
            Self::ManifestNotFound(_) => "manifest_not_found",
            Self::NetworkError(_) => "network_error",
            Self::Internal(_) => "internal_error",
        }
    }

    /// Whether the caller should retry this operation (with backoff).
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::CodespaceStartTimeout { .. }
                | Self::CodespaceUnreachable(_)
                | Self::CommandTimeout { .. }
                | Self::NetworkError(_)
        )
    }

    /// Suggested next action for the caller (agent or human).
    pub fn suggested_action(&self) -> &'static str {
        match self {
            Self::BinaryMissing(_) => "Install the binary or set CODESPACECTL_GH_BIN env var",
            Self::BinaryHashMismatch { .. } => "Re-download the binary from a trusted source",
            Self::AuthFailed(_) => "Regenerate the GitHub PAT and re-export CODESPACECTL_TOKEN",
            Self::TokenRevoked => "Regenerate the GitHub PAT and re-export CODESPACECTL_TOKEN",
            Self::TokenInvalidScope { .. } => "Regenerate PAT with 'codespace' (and 'repo' if pushing) scope",
            Self::TokenMissing => "Set CODESPACECTL_TOKEN env var or run `codespacectl token set`",
            Self::CodespaceNotFound(_) => "Check the codespace name, or run `codespacectl discover`",
            Self::CodespaceStartTimeout { .. } => "Retry with --timeout 600, or check status.github.com",
            Self::CodespaceUnreachable(_) => "Check network, or retry with --timeout 600",
            Self::HealthCheckFailed { .. } => "Run `codespacectl doctor` on the codespace, or `codespacectl connect --force`",
            Self::CommandTimeout { .. } => "Increase timeoutSecs in manifest, or run command in chunks",
            Self::CommandFailed { .. } => "Inspect the command output in the session log",
            Self::HostKeyMismatch { .. } => "If codespace was rebuilt, run `codespacectl connect --accept-new-host-key`",
            Self::ManifestInvalid(_) => "Validate CODESPACE.yaml against docs/MANIFEST_SPEC.md",
            Self::ManifestVersionUnsupported(_) => "Upgrade codespacectl, or use apiVersion: v1",
            Self::ManifestNotFound(_) => "Provide path via --manifest, or run `codespacectl init`",
            Self::NetworkError(_) => "Check network and retry",
            Self::Internal(_) => "Report a bug at https://github.com/topic-hash/codespacectl/issues",
        }
    }

    /// Structured context for the error (for the JSON envelope).
    pub fn context(&self) -> Option<serde_json::Value> {
        match self {
            Self::BinaryHashMismatch { expected, actual } => Some(serde_json::json!({
                "expected_sha256": expected,
                "actual_sha256": actual,
            })),
            Self::TokenInvalidScope { scope } => Some(serde_json::json!({
                "missing_scope": scope,
            })),
            Self::CodespaceStartTimeout { elapsed_secs } => Some(serde_json::json!({
                "elapsed_secs": elapsed_secs,
            })),
            Self::HealthCheckFailed { check, exit_code, stderr } => Some(serde_json::json!({
                "check": check,
                "exit_code": exit_code,
                "stderr": stderr,
            })),
            Self::CommandFailed { exit_code, stderr } => Some(serde_json::json!({
                "exit_code": exit_code,
                "stderr": stderr,
            })),
            Self::HostKeyMismatch { expected, actual } => Some(serde_json::json!({
                "expected_fingerprint": expected,
                "actual_fingerprint": actual,
            })),
            _ => None,
        }
    }

    /// Process exit code (sysexits.h semantics).
    pub fn exit_code(&self) -> i32 {
        match self {
            // Config errors
            Self::TokenMissing
            | Self::ManifestInvalid(_)
            | Self::ManifestVersionUnsupported(_)
            | Self::ManifestNotFound(_) => 65,

            // Internal errors
            Self::BinaryMissing(_)
            | Self::BinaryHashMismatch { .. }
            | Self::AuthFailed(_)
            | Self::TokenRevoked
            | Self::TokenInvalidScope { .. }
            | Self::CodespaceNotFound(_)
            | Self::HealthCheckFailed { .. }
            | Self::CommandFailed { .. }
            | Self::Internal(_) => 70,

            // Temporary failures (retry)
            Self::CodespaceStartTimeout { .. }
            | Self::CodespaceUnreachable(_)
            | Self::CommandTimeout { .. }
            | Self::NetworkError(_) => 75,

            // Protocol errors
            Self::HostKeyMismatch { .. } => 76,
        }
    }
}

impl From<std::io::Error> for CodespaceError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound {
            Self::NetworkError(format!("file not found: {}", e))
        } else {
            Self::NetworkError(e.to_string())
        }
    }
}

impl From<serde_json::Error> for CodespaceError {
    fn from(e: serde_json::Error) -> Self {
        Self::Internal(format!("JSON error: {}", e))
    }
}

impl From<serde_yaml::Error> for CodespaceError {
    fn from(e: serde_yaml::Error) -> Self {
        Self::ManifestInvalid(format!("YAML parse error: {}", e))
    }
}

impl From<reqwest::Error> for CodespaceError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Self::CodespaceUnreachable(format!("HTTP timeout: {}", e))
        } else if e.is_connect() {
            Self::CodespaceUnreachable(format!("connect error: {}", e))
        } else {
            Self::NetworkError(e.to_string())
        }
    }
}

/// Serializable error envelope for `--json` output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub kind: String,
    pub message: String,
    pub retryable: bool,
    pub suggested_action: String,
    pub context: Option<serde_json::Value>,
}

impl From<&CodespaceError> for ErrorEnvelope {
    fn from(e: &CodespaceError) -> Self {
        Self {
            kind: e.kind().to_string(),
            message: e.to_string(),
            retryable: e.retryable(),
            suggested_action: e.suggested_action().to_string(),
            context: e.context(),
        }
    }
}

impl fmt::Display for ErrorEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)?;
        if self.retryable {
            write!(f, " (retryable)")?;
        }
        write!(f, "\n  → {}", self.suggested_action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build one instance of every CodespaceError variant so we can
    /// parameterize tests across the full closed set of 18 variants.
    fn all_variants() -> Vec<CodespaceError> {
        vec![
            CodespaceError::BinaryMissing("gh".into()),
            CodespaceError::BinaryHashMismatch {
                expected: "aaa".into(),
                actual: "bbb".into(),
            },
            CodespaceError::AuthFailed("401".into()),
            CodespaceError::TokenRevoked,
            CodespaceError::TokenInvalidScope {
                scope: "repo".into(),
            },
            CodespaceError::TokenMissing,
            CodespaceError::CodespaceNotFound("foo".into()),
            CodespaceError::CodespaceStartTimeout { elapsed_secs: 120 },
            CodespaceError::CodespaceUnreachable("net".into()),
            CodespaceError::HealthCheckFailed {
                check: "db".into(),
                exit_code: 1,
                stderr: "boom".into(),
            },
            CodespaceError::CommandTimeout { timeout_secs: 30 },
            CodespaceError::CommandFailed {
                exit_code: 2,
                stderr: "err".into(),
            },
            CodespaceError::HostKeyMismatch {
                expected: "fp1".into(),
                actual: "fp2".into(),
            },
            CodespaceError::ManifestInvalid("bad".into()),
            CodespaceError::ManifestVersionUnsupported("v2".into()),
            CodespaceError::ManifestNotFound("/x/CODESPACE.yaml".into()),
            CodespaceError::NetworkError("conn refused".into()),
            CodespaceError::Internal("oops".into()),
        ]
    }

    /// Expected `kind()` strings in the same order as `all_variants()`.
    const EXPECTED_KINDS: [&str; 18] = [
        "binary_missing",
        "binary_hash_mismatch",
        "auth_failed",
        "token_revoked",
        "token_invalid_scope",
        "token_missing",
        "codespace_not_found",
        "codespace_start_timeout",
        "codespace_unreachable",
        "health_check_failed",
        "command_timeout",
        "command_failed",
        "host_key_mismatch",
        "manifest_invalid",
        "manifest_version_unsupported",
        "manifest_not_found",
        "network_error",
        "internal_error",
    ];

    /// Expected `exit_code()` per variant in the same order as `all_variants()`.
    const EXPECTED_EXIT_CODES: [i32; 18] = [
        70, // BinaryMissing — internal
        70, // BinaryHashMismatch — internal
        70, // AuthFailed — internal
        70, // TokenRevoked — internal
        70, // TokenInvalidScope — internal
        65, // TokenMissing — config
        70, // CodespaceNotFound — internal
        75, // CodespaceStartTimeout — temp
        75, // CodespaceUnreachable — temp
        70, // HealthCheckFailed — internal
        75, // CommandTimeout — temp
        70, // CommandFailed — internal
        76, // HostKeyMismatch — protocol
        65, // ManifestInvalid — config
        65, // ManifestVersionUnsupported — config
        65, // ManifestNotFound — config
        75, // NetworkError — temp
        70, // Internal — internal
    ];

    /// Per-variant expected retryable flag, in the same order as `all_variants()`.
    const EXPECTED_RETRYABLE: [bool; 18] = [
        false, // BinaryMissing
        false, // BinaryHashMismatch
        false, // AuthFailed
        false, // TokenRevoked
        false, // TokenInvalidScope
        false, // TokenMissing
        false, // CodespaceNotFound
        true,  // CodespaceStartTimeout
        true,  // CodespaceUnreachable
        false, // HealthCheckFailed
        true,  // CommandTimeout
        false, // CommandFailed
        false, // HostKeyMismatch
        false, // ManifestInvalid
        false, // ManifestVersionUnsupported
        false, // ManifestNotFound
        true,  // NetworkError
        false, // Internal
    ];

    #[test]
    fn test_kind_returns_correct_string_for_every_variant() {
        let variants = all_variants();
        assert_eq!(variants.len(), 18, "expected exactly 18 variants");
        for (i, err) in variants.iter().enumerate() {
            assert_eq!(
                err.kind(),
                EXPECTED_KINDS[i],
                "variant #{} ({}) returned wrong kind",
                i,
                EXPECTED_KINDS[i]
            );
        }
    }

    #[test]
    fn test_retryable_returns_true_only_for_designated_variants() {
        let variants = all_variants();
        for (i, err) in variants.iter().enumerate() {
            assert_eq!(
                err.retryable(),
                EXPECTED_RETRYABLE[i],
                "variant #{} ({}) returned wrong retryable",
                i,
                EXPECTED_KINDS[i]
            );
        }
        // Sanity: only 4 should be retryable.
        let retryable_count = variants.iter().filter(|e| e.retryable()).count();
        assert_eq!(
            retryable_count, 4,
            "expected exactly 4 retryable variants, got {}",
            retryable_count
        );
    }

    #[test]
    fn test_exit_code_returns_correct_sysexits_code() {
        let variants = all_variants();
        for (i, err) in variants.iter().enumerate() {
            assert_eq!(
                err.exit_code(),
                EXPECTED_EXIT_CODES[i],
                "variant #{} ({}) returned wrong exit code",
                i,
                EXPECTED_KINDS[i]
            );
        }
    }

    #[test]
    fn test_suggested_action_non_empty_for_every_variant() {
        for err in all_variants() {
            let action = err.suggested_action();
            assert!(
                !action.is_empty(),
                "suggested_action for {} was empty",
                err.kind()
            );
        }
    }

    #[rstest::rstest]
    #[case(CodespaceError::CodespaceStartTimeout { elapsed_secs: 5 })]
    #[case(CodespaceError::CodespaceUnreachable("offline".into()))]
    #[case(CodespaceError::CommandTimeout { timeout_secs: 60 })]
    #[case(CodespaceError::NetworkError("refused".into()))]
    fn test_retryable_variants(#[case] err: CodespaceError) {
        assert!(err.retryable(), "expected {} to be retryable", err.kind());
    }

    #[rstest::rstest]
    #[case(CodespaceError::BinaryMissing("gh".into()))]
    #[case(CodespaceError::BinaryHashMismatch { expected: "a".into(), actual: "b".into() })]
    #[case(CodespaceError::AuthFailed("x".into()))]
    #[case(CodespaceError::TokenRevoked)]
    #[case(CodespaceError::TokenInvalidScope { scope: "repo".into() })]
    #[case(CodespaceError::TokenMissing)]
    #[case(CodespaceError::CodespaceNotFound("x".into()))]
    #[case(CodespaceError::HealthCheckFailed { check: "c".into(), exit_code: 1, stderr: "s".into() })]
    #[case(CodespaceError::CommandFailed { exit_code: 1, stderr: "s".into() })]
    #[case(CodespaceError::HostKeyMismatch { expected: "a".into(), actual: "b".into() })]
    #[case(CodespaceError::ManifestInvalid("x".into()))]
    #[case(CodespaceError::ManifestVersionUnsupported("v2".into()))]
    #[case(CodespaceError::ManifestNotFound("/x".into()))]
    #[case(CodespaceError::Internal("x".into()))]
    fn test_non_retryable_variants(#[case] err: CodespaceError) {
        assert!(!err.retryable(), "expected {} to NOT be retryable", err.kind());
    }

    // -------------------- From<std::io::Error> --------------------

    #[test]
    fn test_from_io_error_not_found_maps_to_network_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: CodespaceError = io_err.into();
        assert_eq!(err.kind(), "network_error");
        // io::ErrorKind::NotFound maps to NetworkError, which IS retryable.
        assert!(err.retryable());
        assert_eq!(err.exit_code(), 75);
        let msg = err.to_string();
        assert!(msg.contains("file not found") || msg.contains("file missing"));
    }

    #[test]
    fn test_from_io_error_permission_denied_maps_to_network_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err: CodespaceError = io_err.into();
        assert_eq!(err.kind(), "network_error");
    }

    #[test]
    fn test_from_io_error_other_kind_maps_to_network_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "boom");
        let err: CodespaceError = io_err.into();
        assert_eq!(err.kind(), "network_error");
    }

    #[rstest::rstest]
    #[case(std::io::ErrorKind::ConnectionRefused)]
    #[case(std::io::ErrorKind::ConnectionReset)]
    #[case(std::io::ErrorKind::ConnectionAborted)]
    #[case(std::io::ErrorKind::NotConnected)]
    #[case(std::io::ErrorKind::BrokenPipe)]
    #[case(std::io::ErrorKind::TimedOut)]
    #[case(std::io::ErrorKind::Interrupted)]
    #[case(std::io::ErrorKind::UnexpectedEof)]
    #[case(std::io::ErrorKind::WriteZero)]
    fn test_from_io_error_various_kinds(#[case] kind: std::io::ErrorKind) {
        let io_err = std::io::Error::new(kind, "msg");
        let err: CodespaceError = io_err.into();
        assert_eq!(err.kind(), "network_error");
    }

    // -------------------- From<serde_json::Error> --------------------

    #[test]
    fn test_from_serde_json_error_produces_internal() {
        let json_err = serde_json::from_str::<serde_json::Value>("{bad json").unwrap_err();
        let err: CodespaceError = json_err.into();
        assert_eq!(err.kind(), "internal_error");
        assert!(!err.retryable());
        assert_eq!(err.exit_code(), 70);
    }

    // -------------------- From<serde_yaml::Error> --------------------

    #[test]
    fn test_from_serde_yaml_error_produces_manifest_invalid() {
        let yaml_err = serde_yaml::from_str::<serde_json::Value>(": : :").unwrap_err();
        let err: CodespaceError = yaml_err.into();
        assert_eq!(err.kind(), "manifest_invalid");
        assert!(!err.retryable());
        assert_eq!(err.exit_code(), 65);
    }

    // -------------------- From<reqwest::Error> --------------------

    /// Construct a real `reqwest::Error` by asking a `Client` to build a
    /// request from a malformed URL. `RequestBuilder::build` runs `IntoUrl`
    /// eagerly and converts the parse failure into a `reqwest::Error` of
    /// kind `InvalidUrl` (not a timeout, not a connect error). This is the
    /// only category the `From<reqwest::Error>` impl classifies as a generic
    /// `NetworkError` — so it's the easiest path to exercise that arm without
    /// standing up a real HTTP server.
    fn make_invalid_url_reqwest_error() -> reqwest::Error {
        let client = reqwest::Client::new();
        client
            .get("not a url at all")
            .build()
            .expect_err("invalid URL should produce a reqwest::Error")
    }

    #[test]
    fn test_from_reqwest_error_invalid_url_maps_to_network_error() {
        let err: CodespaceError = make_invalid_url_reqwest_error().into();
        assert_eq!(err.kind(), "network_error");
        assert!(err.retryable(), "NetworkError is retryable");
        assert_eq!(err.exit_code(), 75);
    }

    #[test]
    fn test_from_reqwest_error_is_not_timeout_nor_connect() {
        let raw = make_invalid_url_reqwest_error();
        assert!(!raw.is_timeout(), "invalid URL error should not be a timeout");
        assert!(!raw.is_connect(), "invalid URL error should not be a connect error");
    }

    // -------------------- ErrorEnvelope --------------------

    #[test]
    fn test_error_envelope_from_codespace_error_round_trips_fields() {
        let err = CodespaceError::CodespaceStartTimeout { elapsed_secs: 42 };
        let env = ErrorEnvelope::from(&err);
        assert_eq!(env.kind, "codespace_start_timeout");
        assert_eq!(env.retryable, true);
        assert!(!env.suggested_action.is_empty());
        // Context should be present (CodespaceStartTimeout has structured context).
        assert!(env.context.is_some(), "context should be Some for CodespaceStartTimeout");
        let ctx = env.context.unwrap();
        assert_eq!(ctx["elapsed_secs"], 42);
    }

    #[test]
    fn test_error_envelope_round_trips_for_non_contextual_variant() {
        let err = CodespaceError::TokenMissing;
        let env = ErrorEnvelope::from(&err);
        assert_eq!(env.kind, "token_missing");
        assert_eq!(env.retryable, false);
        assert!(env.context.is_none(), "TokenMissing has no structured context");
    }

    #[test]
    fn test_error_envelope_serializes_and_deserializes() {
        let err = CodespaceError::HostKeyMismatch {
            expected: "SHA256:aaa".into(),
            actual: "SHA256:bbb".into(),
        };
        let env = ErrorEnvelope::from(&err);
        let json = serde_json::to_string(&env).expect("serialize");
        let back: ErrorEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.kind, env.kind);
        assert_eq!(back.message, env.message);
        assert_eq!(back.retryable, env.retryable);
        assert_eq!(back.suggested_action, env.suggested_action);
        assert_eq!(back.context, env.context);
    }

    #[test]
    fn test_error_envelope_round_trips_for_all_variants() {
        for err in all_variants() {
            let env = ErrorEnvelope::from(&err);
            let json = serde_json::to_string(&env).expect("serialize");
            let back: ErrorEnvelope = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back.kind, env.kind);
            assert_eq!(back.retryable, env.retryable);
            assert_eq!(back.suggested_action, env.suggested_action);
            assert_eq!(back.context, env.context);
        }
    }

    #[test]
    fn test_error_envelope_display_format() {
        let err = CodespaceError::NetworkError("refused".into());
        let env = ErrorEnvelope::from(&err);
        let s = format!("{}", env);
        assert!(s.contains("[network_error]"), "display should include kind, got: {}", s);
        assert!(s.contains("refused"), "display should include message, got: {}", s);
        assert!(s.contains("(retryable)"), "display should mark retryable, got: {}", s);
        assert!(s.contains("→"), "display should include suggested action arrow, got: {}", s);
    }

    #[test]
    fn test_error_envelope_display_non_retryable_no_marker() {
        let err = CodespaceError::TokenMissing;
        let env = ErrorEnvelope::from(&err);
        let s = format!("{}", env);
        assert!(!s.contains("(retryable)"), "non-retryable display should not include marker, got: {}", s);
    }

    // -------------------- context() --------------------

    #[test]
    fn test_context_returns_none_for_simple_variants() {
        let simple_variants = vec![
            CodespaceError::BinaryMissing("x".into()),
            CodespaceError::AuthFailed("x".into()),
            CodespaceError::TokenRevoked,
            CodespaceError::TokenMissing,
            CodespaceError::CodespaceNotFound("x".into()),
            CodespaceError::CodespaceUnreachable("x".into()),
            CodespaceError::ManifestInvalid("x".into()),
            CodespaceError::ManifestVersionUnsupported("v2".into()),
            CodespaceError::ManifestNotFound("/x".into()),
            CodespaceError::NetworkError("x".into()),
            CodespaceError::Internal("x".into()),
        ];
        for err in simple_variants {
            assert!(err.context().is_none(), "{} should have no context", err.kind());
        }
    }

    #[test]
    fn test_context_returns_some_for_structured_variants() {
        let structured_variants: Vec<CodespaceError> = vec![
            CodespaceError::BinaryHashMismatch { expected: "a".into(), actual: "b".into() },
            CodespaceError::TokenInvalidScope { scope: "repo".into() },
            CodespaceError::CodespaceStartTimeout { elapsed_secs: 5 },
            CodespaceError::HealthCheckFailed { check: "c".into(), exit_code: 1, stderr: "s".into() },
            CodespaceError::CommandFailed { exit_code: 1, stderr: "s".into() },
            CodespaceError::HostKeyMismatch { expected: "a".into(), actual: "b".into() },
        ];
        for err in structured_variants {
            assert!(err.context().is_some(), "{} should have context", err.kind());
        }
    }

    #[test]
    fn test_context_binary_hash_mismatch_fields() {
        let err = CodespaceError::BinaryHashMismatch {
            expected: "abc".into(),
            actual: "def".into(),
        };
        let ctx = err.context().unwrap();
        assert_eq!(ctx["expected_sha256"], "abc");
        assert_eq!(ctx["actual_sha256"], "def");
    }

    #[test]
    fn test_context_health_check_failed_fields() {
        let err = CodespaceError::HealthCheckFailed {
            check: "db_ready".into(),
            exit_code: 1,
            stderr: "boom".into(),
        };
        let ctx = err.context().unwrap();
        assert_eq!(ctx["check"], "db_ready");
        assert_eq!(ctx["exit_code"], 1);
        assert_eq!(ctx["stderr"], "boom");
    }

    #[test]
    fn test_context_command_failed_fields() {
        let err = CodespaceError::CommandFailed {
            exit_code: 2,
            stderr: "err".into(),
        };
        let ctx = err.context().unwrap();
        assert_eq!(ctx["exit_code"], 2);
        assert_eq!(ctx["stderr"], "err");
    }

    #[test]
    fn test_context_host_key_mismatch_fields() {
        let err = CodespaceError::HostKeyMismatch {
            expected: "fp1".into(),
            actual: "fp2".into(),
        };
        let ctx = err.context().unwrap();
        assert_eq!(ctx["expected_fingerprint"], "fp1");
        assert_eq!(ctx["actual_fingerprint"], "fp2");
    }

    #[test]
    fn test_context_token_invalid_scope_fields() {
        let err = CodespaceError::TokenInvalidScope { scope: "repo".into() };
        let ctx = err.context().unwrap();
        assert_eq!(ctx["missing_scope"], "repo");
    }

    #[test]
    fn test_context_codespace_start_timeout_fields() {
        let err = CodespaceError::CodespaceStartTimeout { elapsed_secs: 99 };
        let ctx = err.context().unwrap();
        assert_eq!(ctx["elapsed_secs"], 99);
    }
}
