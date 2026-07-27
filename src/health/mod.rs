//! Health check runner. Executes manifest-declared health checks on the codespace.
//!
//! A "health check" is a single shell command declared in the manifest under
//! `environment.healthChecks`. Each check carries an `expectExitCode` (default
//! 0) and a `timeoutSecs` (default 30). We render the command via the
//! manifest template renderer (so `{{workingDir}}`/`{{secret.NAME}}` work),
//! ship it to the codespace via `ssh.exec`, time the round trip, and compare
//! the remote exit code against the expected one.
//!
//! Error policy:
//! - Soft failures (the command itself failed, timed out, or returned the
//!   wrong exit code) are folded into a `HealthCheckResult { passed: false }`
//!   so the caller can still collect partial results from the remaining
//!   checks.
//! - Hard failures (the SSH transport is gone — e.g. `TransportClosed`,
//!   `CodespaceUnreachable`, `HostKeyMismatch`) propagate as `Result::Err`
//!   so the caller knows to stop iterating and reconnect.

use crate::manifest::{render_template, HealthCheck, TemplateContext};
use crate::ssh::CodespaceSsh;
use crate::{CodespaceError, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Sentinel exit code used when an SSH exec never produced an exit status
/// (timeout, channel closed, transport error). Negative so it can never
/// collide with a real Unix exit code (which is 0–255).
const NO_EXIT_CODE: i32 = -1;

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
///
/// Renders `check.command` against `ctx` (so manifest template placeholders
/// like `{{workingDir}}` and `{{secret.NAME}}` are substituted before exec),
/// then ships it to the codespace via `ssh.exec` with a per-check timeout.
///
/// Returns `Ok(HealthCheckResult)` for both passing and soft-failing checks
/// (wrong exit code, command timeout, exec error). Returns `Err` only when
/// the SSH session itself is dead (transport closed, unreachable, host key
/// mismatch) — in that case the caller should stop iterating and reconnect.
pub async fn run_check(
    ssh: &mut CodespaceSsh,
    check: &HealthCheck,
    ctx: &TemplateContext,
) -> Result<HealthCheckResult> {
    let command = render_template(&check.command, ctx);
    let timeout = Duration::from_secs(check.timeout_secs);
    let started = Instant::now();

    let exec_result = ssh.exec(&command, timeout).await;
    let duration_secs = started.elapsed().as_secs_f64();

    match exec_result {
        Ok(result) => {
            let passed = result.exit_code == check.expect_exit_code;
            Ok(HealthCheckResult {
                name: check.name.clone(),
                passed,
                exit_code: result.exit_code,
                stdout: result.stdout,
                stderr: result.stderr,
                duration_secs,
            })
        }
        Err(e) => {
            // Hard errors mean the SSH session itself is unusable: propagate
            // them so the caller knows to stop and reconnect. Soft errors
            // (timeout, exec failure, "channel closed without exit status")
            // get folded into a passed=false HealthCheckResult so partial
            // results can still be collected from the remaining checks.
            if is_fatal_ssh_error(&e) {
                return Err(e);
            }

            Ok(HealthCheckResult {
                name: check.name.clone(),
                passed: false,
                exit_code: NO_EXIT_CODE,
                stdout: String::new(),
                stderr: e.to_string(),
                duration_secs,
            })
        }
    }
}

/// Run all health checks from a manifest.
///
/// Iterates `checks` in declaration order, calling `run_check` for each. If
/// any individual check returns a fatal SSH error (transport closed, etc.),
/// iteration stops immediately and that error is propagated — the remaining
/// checks are skipped because the SSH session is no longer usable.
///
/// Otherwise the collected results are passed through `build_report` to
/// produce the final `HealthReport`.
pub async fn run_all_checks(
    ssh: &mut CodespaceSsh,
    checks: &[HealthCheck],
    ctx: &TemplateContext,
) -> Result<HealthReport> {
    let mut results: Vec<HealthCheckResult> = Vec::with_capacity(checks.len());
    for check in checks {
        let result = run_check(ssh, check, ctx).await?;
        results.push(result);
    }
    Ok(build_report(results))
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

/// Classify whether an error from `ssh.exec` means the SSH session is dead
/// and the caller should stop iterating checks.
///
/// `ssh::transport::exec` currently surfaces two `SshError` variants, both
/// of which convert into `CodespaceError::Internal(...)`:
/// - `SshError::ExecFailed` (channel/timeout/exec issues) — soft, fold in.
/// - `SshError::TransportClosed` ("transport closed unexpectedly") — hard.
///
/// The other `CodespaceError` variants below (`CodespaceUnreachable`,
/// `HostKeyMismatch`, `NetworkError`) don't currently come out of `exec` but
/// are classified as fatal here for robustness — if the transport layer ever
/// starts surfacing them, the right behavior is to stop and reconnect.
fn is_fatal_ssh_error(e: &CodespaceError) -> bool {
    match e {
        CodespaceError::CodespaceUnreachable(_) => true,
        CodespaceError::HostKeyMismatch { .. } => true,
        CodespaceError::NetworkError(_) => true,
        CodespaceError::Internal(msg) => msg.contains("transport closed"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `HealthCheckResult` for testing with reasonable defaults.
    fn make_result(name: &str, passed: bool, duration_secs: f64) -> HealthCheckResult {
        let exit_code = if passed { 0 } else { 1 };
        HealthCheckResult {
            name: name.to_string(),
            passed,
            exit_code,
            stdout: String::new(),
            stderr: String::new(),
            duration_secs,
        }
    }

    #[test]
    fn test_build_report_all_pass() {
        let results = vec![
            make_result("disk_space", true, 0.12),
            make_result("db_up", true, 0.34),
            make_result("api_responds", true, 0.56),
        ];
        let report = build_report(results);
        assert_eq!(report.overall, HealthStatus::Green);
        assert_eq!(report.checks.len(), 3);
        // All individual checks should be marked passing.
        assert!(report.checks.iter().all(|r| r.passed));
    }

    #[test]
    fn test_build_report_some_fail() {
        let results = vec![
            make_result("disk_space", true, 0.12),
            make_result("db_up", true, 0.34),
            make_result("api_responds", false, 0.56),
        ];
        let report = build_report(results);
        assert_eq!(report.overall, HealthStatus::Red);
        assert_eq!(report.checks.len(), 3);
        // Exactly one check failed.
        assert_eq!(report.checks.iter().filter(|r| !r.passed).count(), 1);
    }

    #[test]
    fn test_build_report_empty() {
        let results: Vec<HealthCheckResult> = vec![];
        let report = build_report(results);
        // Empty list: trivially all-passing (the `all` iterator on an empty
        // vec returns true), so overall is Green.
        assert_eq!(report.overall, HealthStatus::Green);
        assert!(report.checks.is_empty());
    }

    #[test]
    fn test_build_report_records_check_name() {
        let results = vec![make_result("my-special-check", true, 0.42)];
        let report = build_report(results);
        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.checks[0].name, "my-special-check");
    }

    #[test]
    fn test_build_report_records_duration() {
        let results = vec![make_result("timed_check", true, 1.234)];
        let report = build_report(results);
        assert_eq!(report.checks.len(), 1);
        // duration_secs must be non-negative and round-trip exactly.
        assert!(report.checks[0].duration_secs >= 0.0);
        assert!(
            (report.checks[0].duration_secs - 1.234).abs() < 1e-9,
            "duration_secs should round-trip, got {}",
            report.checks[0].duration_secs
        );
    }

    #[test]
    fn test_build_report_includes_checked_at() {
        let results = vec![make_result("check", true, 0.1)];
        let report = build_report(results);
        // checked_at must be a non-empty ISO 8601 / RFC 3339 timestamp like
        // "2026-07-14T12:34:56.789+00:00".
        assert!(
            !report.checked_at.is_empty(),
            "checked_at must not be empty"
        );
        assert!(
            report.checked_at.contains('T'),
            "checked_at should be ISO 8601 (contain 'T' separator), got: {}",
            report.checked_at
        );
        // Either 'Z' (UTC Zulu) or a numeric offset like '+00:00' must be
        // present to be a valid RFC 3339 timestamp.
        let has_tz = report.checked_at.ends_with('Z')
            || report.checked_at.rfind('+').is_some()
            || report.checked_at.rfind('-').is_some_and(|i| i > 10);
        assert!(
            has_tz,
            "checked_at should include a timezone designator, got: {}",
            report.checked_at
        );
    }

    // -----------------------------------------------------------------------
    // build_report: all checks failing → Red.
    // -----------------------------------------------------------------------
    #[test]
    fn test_build_report_all_fail_returns_red() {
        let results = vec![
            make_result("disk_space", false, 0.12),
            make_result("db_up", false, 0.34),
            make_result("api_responds", false, 0.56),
        ];
        let report = build_report(results);
        assert_eq!(report.overall, HealthStatus::Red);
        assert_eq!(report.checks.len(), 3);
        assert!(report.checks.iter().all(|r| !r.passed));
    }

    // -----------------------------------------------------------------------
    // build_report: mixed pass/fail → Red (any single failure means Red).
    // -----------------------------------------------------------------------
    #[test]
    fn test_build_report_mixed_returns_red() {
        let results = vec![
            make_result("ok1", true, 0.1),
            make_result("ok2", true, 0.1),
            make_result("bad1", false, 0.1),
            make_result("ok3", true, 0.1),
        ];
        let report = build_report(results);
        assert_eq!(report.overall, HealthStatus::Red);
        assert_eq!(report.checks.len(), 4);
        assert_eq!(report.checks.iter().filter(|r| r.passed).count(), 3);
        assert_eq!(report.checks.iter().filter(|r| !r.passed).count(), 1);
    }

    // -----------------------------------------------------------------------
    // build_report: non-empty all-passing → Green (not Red).
    // -----------------------------------------------------------------------
    #[test]
    fn test_build_report_non_empty_all_pass_returns_green() {
        let results = vec![
            make_result("a", true, 0.1),
            make_result("b", true, 0.1),
            make_result("c", true, 0.1),
        ];
        let report = build_report(results);
        assert_eq!(report.overall, HealthStatus::Green);
        assert_eq!(report.checks.len(), 3);
    }

    // -----------------------------------------------------------------------
    // build_report: exit codes round-trip into the report.
    // -----------------------------------------------------------------------
    #[test]
    fn test_build_report_records_exit_code() {
        let mut r1 = make_result("ok", true, 0.1);
        r1.exit_code = 0;
        let mut r2 = make_result("fail", false, 0.1);
        r2.exit_code = 42;
        let report = build_report(vec![r1, r2]);
        assert_eq!(report.checks[0].exit_code, 0);
        assert_eq!(report.checks[1].exit_code, 42);
    }

    // -----------------------------------------------------------------------
    // HealthStatus: Display formats as "green"/"red".
    // -----------------------------------------------------------------------
    #[test]
    fn test_health_status_display() {
        assert_eq!(HealthStatus::Green.to_string(), "green");
        assert_eq!(HealthStatus::Red.to_string(), "red");
    }

    // -----------------------------------------------------------------------
    // HealthStatus: serializes to lowercase strings.
    // -----------------------------------------------------------------------
    #[test]
    fn test_health_status_serializes_to_lowercase() {
        assert_eq!(
            serde_json::to_string(&HealthStatus::Green).unwrap(),
            "\"green\""
        );
        assert_eq!(
            serde_json::to_string(&HealthStatus::Red).unwrap(),
            "\"red\""
        );
    }

    // -----------------------------------------------------------------------
    // HealthStatus: deserializes from lowercase strings.
    // -----------------------------------------------------------------------
    #[test]
    fn test_health_status_deserializes_from_lowercase() {
        let g: HealthStatus = serde_json::from_str("\"green\"").unwrap();
        assert_eq!(g, HealthStatus::Green);
        let r: HealthStatus = serde_json::from_str("\"red\"").unwrap();
        assert_eq!(r, HealthStatus::Red);
    }

    // -----------------------------------------------------------------------
    // HealthCheckResult: serializes to JSON with the expected fields.
    // -----------------------------------------------------------------------
    #[test]
    fn test_health_check_result_serializes_correctly() {
        let result = HealthCheckResult {
            name: "disk_space".to_string(),
            passed: true,
            exit_code: 0,
            stdout: "ok".to_string(),
            stderr: String::new(),
            duration_secs: 0.123,
        };
        let json = serde_json::to_value(&result).expect("serialize HealthCheckResult");
        assert_eq!(json["name"], "disk_space");
        assert_eq!(json["passed"], true);
        assert_eq!(json["exit_code"], 0);
        assert_eq!(json["stdout"], "ok");
        assert_eq!(json["stderr"], "");
        assert_eq!(json["duration_secs"], 0.123);
    }

    // -----------------------------------------------------------------------
    // HealthReport: serializes to JSON with the expected fields.
    // -----------------------------------------------------------------------
    #[test]
    fn test_health_report_serializes_correctly() {
        let report = HealthReport {
            overall: HealthStatus::Green,
            checks: vec![HealthCheckResult {
                name: "a".to_string(),
                passed: true,
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                duration_secs: 0.5,
            }],
            checked_at: "2026-07-14T12:00:00+00:00".to_string(),
        };
        let json = serde_json::to_value(&report).expect("serialize HealthReport");
        assert_eq!(json["overall"], "green");
        assert_eq!(json["checked_at"], "2026-07-14T12:00:00+00:00");
        assert_eq!(json["checks"][0]["name"], "a");
        assert_eq!(json["checks"][0]["passed"], true);
    }

    // -----------------------------------------------------------------------
    // HealthReport: round-trips through serialize → deserialize unchanged.
    // -----------------------------------------------------------------------
    #[test]
    fn test_health_report_round_trip() {
        let report = HealthReport {
            overall: HealthStatus::Red,
            checks: vec![
                HealthCheckResult {
                    name: "ok".to_string(),
                    passed: true,
                    exit_code: 0,
                    stdout: "out".to_string(),
                    stderr: String::new(),
                    duration_secs: 0.1,
                },
                HealthCheckResult {
                    name: "fail".to_string(),
                    passed: false,
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: "boom".to_string(),
                    duration_secs: 0.2,
                },
            ],
            checked_at: "2026-07-14T12:00:00+00:00".to_string(),
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: HealthReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.overall, report.overall);
        assert_eq!(back.checks.len(), report.checks.len());
        assert_eq!(back.checks[0].name, "ok");
        assert_eq!(back.checks[1].name, "fail");
        assert_eq!(back.checks[1].stderr, "boom");
        assert_eq!(back.checked_at, report.checked_at);
    }
}
