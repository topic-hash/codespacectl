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
