//! Health check runner. Executes manifest-declared health checks on the codespace.

use crate::manifest::HealthCheck;
use crate::ssh::CodespaceSsh;
use crate::{CodespaceError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Result of a single health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub name: String,
    pub passed: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_secs: f64,
}

/// Aggregated result of all health checks for a codespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub overall: HealthStatus,
    pub checks: Vec<HealthCheckResult>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Green,
    Red,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Green => write!(f, "green"),
            Self::Red => write!(f, "red"),
        }
    }
}

/// Run a single health check via SSH.
pub async fn run_check(
    ssh: &mut CodespaceSsh,
    check: &HealthCheck,
) -> Result<HealthCheckResult> {
    let _ = (ssh, check);
    // TODO (Wave 6 subagent): implement
    Err(CodespaceError::Internal(
        "run_check not yet implemented — Wave 6 subagent pending".into(),
    ))
}

/// Run all health checks from a manifest.
pub async fn run_all_checks(
    ssh: &mut CodespaceSsh,
    checks: &[HealthCheck],
) -> Result<HealthReport> {
    let _ = (ssh, checks);
    // TODO (Wave 6 subagent): implement
    Err(CodespaceError::Internal(
        "run_all_checks not yet implemented — Wave 6 subagent pending".into(),
    ))
}

/// Helper: build a HealthReport from results.
pub fn build_report(results: Vec<HealthCheckResult>) -> HealthReport {
    let overall = if results.iter().all(|r| r.passed) {
        HealthStatus::Green
    } else {
        HealthStatus::Red
    };
    HealthReport {
        overall,
        checks: results,
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}
