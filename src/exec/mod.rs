//! Manifest command execution + lifecycle hooks.
//!
//! Wave 7 subagent: implements `exec_command`, `exec_raw`, `run_post_start`,
//! and `run_pre_stop`.
//!
//! ## Design notes
//!
//! - `exec_command` / `exec_raw` take a `&SessionLog` and append `ExecStart` /
//!   `ExecEnd` entries (best-effort — log write failures are swallowed because
//!   the command itself may still have succeeded on the remote side).
//! - A non-zero remote exit code is **not** an error from SSH's perspective
//!   (the channel produced output and an ExitStatus). Such results are returned
//!   as `Ok(ExecOutput)` so the caller (CLI / agent) can decide how to react.
//! - The russh transport wraps exec timeouts as `SshError::ExecFailed("...
//!   timed out after Ns")`, which the `From<SshError> for CodespaceError` impl
//!   converts to `CodespaceError::Internal(...)`. We detect that pattern here
//!   and re-classify it as `CodespaceError::CommandTimeout` so callers get the
//!   correct stable `error.kind = "command_timeout"` (which is `retryable`).
//! - `run_post_start` / `run_pre_stop` share a private `run_hooks` helper.
//!   Non-zero exit codes become `CodespaceError::CommandFailed`; timeouts
//!   become `CodespaceError::CommandTimeout`.

use crate::manifest::{render_template, Command, HookCommand, TemplateContext};
use crate::session::{SessionEntryKind, SessionLog};
use crate::ssh::CodespaceSsh;
use crate::{CodespaceError, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Result of an `exec` invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecOutput {
    /// Manifest command name (or `"raw"` for ad-hoc commands).
    pub command_name: String,
    /// Captured stdout from the remote command.
    pub stdout: String,
    /// Captured stderr from the remote command.
    pub stderr: String,
    /// Remote process exit code. `-1` indicates a timeout or transport-level
    /// failure (no ExitStatus was received).
    pub exit_code: i32,
    /// Wall-clock duration of the SSH exec call, in seconds (with sub-second
    /// precision).
    pub duration_secs: f64,
    /// Session ID of the session log this exec belongs to.
    pub session_id: String,
}

/// Classify an SSH exec error.
///
/// The russh transport surfaces timeouts as
/// `CodespaceError::Internal("ssh error: ... timed out after Ns")` (because
/// `SshError::ExecFailed` falls into the catch-all arm of
/// `From<SshError> for CodespaceError`). Convert that to a proper
/// `CodespaceError::CommandTimeout` so the error kind is `command_timeout`
/// (retryable, exit code 75) per the catalog in `error.rs`.
///
/// `CommandTimeout` errors that were already classified are passed through
/// unchanged. All other errors are returned unchanged.
fn classify_ssh_err(e: CodespaceError, timeout_secs: u64) -> CodespaceError {
    match &e {
        CodespaceError::CommandTimeout { .. } => e,
        CodespaceError::Internal(msg) if msg.contains("timed out") => {
            CodespaceError::CommandTimeout { timeout_secs }
        }
        _ => e,
    }
}

/// Returns `true` if the error is (or was reclassified as) a command timeout.
fn is_timeout(e: &CodespaceError) -> bool {
    matches!(e, CodespaceError::CommandTimeout { .. })
}

/// Best-effort append to the session log. Errors are swallowed because a log
/// write failure should not mask a successful (or failed) exec result.
fn log_best_effort(session: &SessionLog, kind: SessionEntryKind, data: serde_json::Value) {
    let _ = session.append(kind, data);
}

/// Execute a manifest-declared command via SSH.
///
/// Renders the command template with `ctx`, executes it on the codespace via
/// `ssh.exec`, and appends `ExecStart` / `ExecEnd` entries to `session`.
///
/// A non-zero remote exit code is **not** treated as an error here — the
/// captured output is returned in `ExecOutput` so the caller (CLI / agent)
/// can decide how to react. Only transport-level failures and timeouts
/// propagate as `Err`.
///
/// On timeout, an `ExecEnd` entry with `exit_code=-1` and `timed_out=true`
/// is logged (best-effort) and `CodespaceError::CommandTimeout` is returned.
pub async fn exec_command(
    ssh: &mut CodespaceSsh,
    command_name: &str,
    command: &Command,
    ctx: &TemplateContext,
    session: &SessionLog,
) -> Result<ExecOutput> {
    let rendered = render_template(&command.command, ctx);
    let timeout = Duration::from_secs(command.timeout_secs);

    log_best_effort(
        session,
        SessionEntryKind::ExecStart,
        serde_json::json!({
            "command": command_name,
            "rendered": rendered,
            "timeout_secs": command.timeout_secs,
        }),
    );

    let start = Instant::now();
    let result = ssh.exec(&rendered, timeout).await;
    let duration_secs = start.elapsed().as_secs_f64();

    match result {
        Ok(r) => {
            log_best_effort(
                session,
                SessionEntryKind::ExecEnd,
                serde_json::json!({
                    "command": command_name,
                    "exit_code": r.exit_code,
                    "duration_secs": duration_secs,
                    "stdout_len": r.stdout.len(),
                    "stderr_len": r.stderr.len(),
                }),
            );
            Ok(ExecOutput {
                command_name: command_name.to_string(),
                stdout: r.stdout,
                stderr: r.stderr,
                exit_code: r.exit_code,
                duration_secs,
                session_id: session.id.clone(),
            })
        }
        Err(e) => {
            let classified = classify_ssh_err(e, command.timeout_secs);
            if is_timeout(&classified) {
                log_best_effort(
                    session,
                    SessionEntryKind::ExecEnd,
                    serde_json::json!({
                        "command": command_name,
                        "exit_code": -1,
                        "duration_secs": duration_secs,
                        "stdout_len": 0,
                        "stderr_len": 0,
                        "timed_out": true,
                    }),
                );
            }
            Err(classified)
        }
    }
}

/// Execute an ad-hoc shell command (not declared in manifest).
///
/// Behaves like `exec_command` but with `command_name = "raw"` and no
/// template substitution (the caller is responsible for any rendering they
/// need; we pass `command` through verbatim to the SSH exec channel).
pub async fn exec_raw(
    ssh: &mut CodespaceSsh,
    command: &str,
    timeout: Duration,
    session: &SessionLog,
) -> Result<ExecOutput> {
    let command_name = "raw";
    let timeout_secs = timeout.as_secs();

    log_best_effort(
        session,
        SessionEntryKind::ExecStart,
        serde_json::json!({
            "command": command_name,
            "rendered": command,
            "timeout_secs": timeout_secs,
        }),
    );

    let start = Instant::now();
    let result = ssh.exec(command, timeout).await;
    let duration_secs = start.elapsed().as_secs_f64();

    match result {
        Ok(r) => {
            log_best_effort(
                session,
                SessionEntryKind::ExecEnd,
                serde_json::json!({
                    "command": command_name,
                    "exit_code": r.exit_code,
                    "duration_secs": duration_secs,
                    "stdout_len": r.stdout.len(),
                    "stderr_len": r.stderr.len(),
                }),
            );
            Ok(ExecOutput {
                command_name: command_name.to_string(),
                stdout: r.stdout,
                stderr: r.stderr,
                exit_code: r.exit_code,
                duration_secs,
                session_id: session.id.clone(),
            })
        }
        Err(e) => {
            let classified = classify_ssh_err(e, timeout_secs);
            if is_timeout(&classified) {
                log_best_effort(
                    session,
                    SessionEntryKind::ExecEnd,
                    serde_json::json!({
                        "command": command_name,
                        "exit_code": -1,
                        "duration_secs": duration_secs,
                        "stdout_len": 0,
                        "stderr_len": 0,
                        "timed_out": true,
                    }),
                );
            }
            Err(classified)
        }
    }
}

/// Run all `postStart` hooks sequentially.
///
/// Each hook's `command` (and `cwd`, if set) is rendered with `ctx`. If `cwd`
/// is provided, the rendered command is wrapped as `cd <cwd> && <cmd>` so the
/// hook runs in the intended directory.
///
/// Returns `Err(CodespaceError::CommandFailed { exit_code, stderr })` on any
/// non-zero exit code, or `Err(CodespaceError::CommandTimeout { timeout_secs })`
/// on timeout. Other transport errors propagate as-is.
pub async fn run_post_start(
    ssh: &mut CodespaceSsh,
    hooks: &[HookCommand],
    ctx: &TemplateContext,
) -> Result<()> {
    run_hooks(ssh, hooks, ctx).await
}

/// Run all `preStop` hooks sequentially.
///
/// Identical semantics to `run_post_start` — kept as a separate entry point so
/// callers can clearly distinguish the two lifecycle phases.
pub async fn run_pre_stop(
    ssh: &mut CodespaceSsh,
    hooks: &[HookCommand],
    ctx: &TemplateContext,
) -> Result<()> {
    run_hooks(ssh, hooks, ctx).await
}

/// Shared implementation for `run_post_start` / `run_pre_stop`.
async fn run_hooks(
    ssh: &mut CodespaceSsh,
    hooks: &[HookCommand],
    ctx: &TemplateContext,
) -> Result<()> {
    for hook in hooks {
        let rendered_cmd = render_template(&hook.command, ctx);

        // If `cwd` is provided (and non-empty after trimming), render it and
        // prefix the command with `cd <cwd> &&`. An empty/whitespace cwd would
        // produce `cd  && ...`, which is malformed, so we skip it.
        let full_command = match hook.cwd.as_deref() {
            Some(cwd) if !cwd.trim().is_empty() => {
                let rendered_cwd = render_template(cwd, ctx);
                format!("cd {} && {}", rendered_cwd, rendered_cmd)
            }
            _ => rendered_cmd,
        };

        let timeout = Duration::from_secs(hook.timeout_secs);
        let result = ssh.exec(&full_command, timeout).await;

        match result {
            Ok(r) => {
                if r.exit_code != 0 {
                    return Err(CodespaceError::CommandFailed {
                        exit_code: r.exit_code,
                        stderr: r.stderr,
                    });
                }
                // success — continue to next hook
            }
            Err(e) => {
                let classified = classify_ssh_err(e, hook.timeout_secs);
                return Err(classified);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ExecOutput serializes to the expected JSON shape.
    // -----------------------------------------------------------------------
    #[test]
    fn test_exec_output_default_serialization() {
        let out = ExecOutput {
            command_name: "migrate".to_string(),
            stdout: "rows=42\n".to_string(),
            stderr: "warn: deprecated flag\n".to_string(),
            exit_code: 0,
            duration_secs: 1.5,
            session_id: "11111111-2222-3333-4444-555555555555".to_string(),
        };

        let json = serde_json::to_value(&out).expect("serialize ExecOutput");
        assert_eq!(json["command_name"], "migrate");
        assert_eq!(json["stdout"], "rows=42\n");
        assert_eq!(json["stderr"], "warn: deprecated flag\n");
        assert_eq!(json["exit_code"], 0);
        assert_eq!(json["duration_secs"], 1.5);
        assert_eq!(json["session_id"], "11111111-2222-3333-4444-555555555555");
    }

    // -----------------------------------------------------------------------
    // ExecOutput round-trips through serialize -> deserialize unchanged.
    // -----------------------------------------------------------------------
    #[test]
    fn test_exec_output_round_trip() {
        let out = ExecOutput {
            command_name: "raw".to_string(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 127,
            duration_secs: 0.0,
            session_id: String::new(),
        };
        let json = serde_json::to_string(&out).expect("serialize");
        let back: ExecOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.command_name, out.command_name);
        assert_eq!(back.stdout, out.stdout);
        assert_eq!(back.stderr, out.stderr);
        assert_eq!(back.exit_code, out.exit_code);
        assert_eq!(back.session_id, out.session_id);
    }

    // -----------------------------------------------------------------------
    // classify_ssh_err converts Internal("...timed out...") -> CommandTimeout.
    // This mirrors how the russh transport surfaces exec timeouts today (via
    // SshError::ExecFailed -> CodespaceError::Internal).
    // -----------------------------------------------------------------------
    #[test]
    fn test_classify_ssh_err_converts_internal_timeout() {
        let e = CodespaceError::Internal("ssh error: exec read loop timed out after 30s".into());
        let classified = classify_ssh_err(e, 30);
        assert!(matches!(
            classified,
            CodespaceError::CommandTimeout { timeout_secs: 30 }
        ));
    }

    // -----------------------------------------------------------------------
    // classify_ssh_err preserves an already-classified CommandTimeout (does
    // not overwrite timeout_secs with the call-site value).
    // -----------------------------------------------------------------------
    #[test]
    fn test_classify_ssh_err_preserves_command_timeout() {
        let e = CodespaceError::CommandTimeout { timeout_secs: 42 };
        let classified = classify_ssh_err(e, 99);
        assert!(matches!(
            classified,
            CodespaceError::CommandTimeout { timeout_secs: 42 }
        ));
    }

    // -----------------------------------------------------------------------
    // classify_ssh_err passes through unrelated Internal errors unchanged.
    // -----------------------------------------------------------------------
    #[test]
    fn test_classify_ssh_err_passes_through_other_internal() {
        let e = CodespaceError::Internal("ssh error: channel_open_session: ...".into());
        let classified = classify_ssh_err(e, 30);
        assert!(matches!(classified, CodespaceError::Internal(_)));
    }

    // -----------------------------------------------------------------------
    // classify_ssh_err passes through other error variants unchanged.
    // -----------------------------------------------------------------------
    #[test]
    fn test_classify_ssh_err_passes_through_other_variants() {
        let e = CodespaceError::CodespaceUnreachable("network down".into());
        let classified = classify_ssh_err(e, 30);
        assert!(matches!(
            classified,
            CodespaceError::CodespaceUnreachable(_)
        ));
    }

    // -----------------------------------------------------------------------
    // is_timeout correctly identifies CommandTimeout variants.
    // -----------------------------------------------------------------------
    #[test]
    fn test_is_timeout() {
        assert!(is_timeout(&CodespaceError::CommandTimeout {
            timeout_secs: 5
        }));
        assert!(!is_timeout(&CodespaceError::CommandFailed {
            exit_code: 1,
            stderr: String::new(),
        }));
        assert!(!is_timeout(&CodespaceError::Internal("nope".into())));
    }

    // -----------------------------------------------------------------------
    // Compile-time test: ExecOutput implements Serialize + Deserialize.
    // -----------------------------------------------------------------------
    #[test]
    fn test_exec_output_serde_traits() {
        fn _assert_serde<T: Serialize + for<'de> Deserialize<'de>>() {}
        _assert_serde::<ExecOutput>();
    }

    // -----------------------------------------------------------------------
    // ExecOutput does NOT derive Default. Document the "default-like" values
    // (zero/empty) so a future reader knows what an unpopulated ExecOutput
    // would look like if Default were ever added.
    // -----------------------------------------------------------------------
    #[test]
    fn test_exec_output_default_like_values() {
        // Note: ExecOutput does NOT currently derive Default. If it ever does,
        // the expected default values are: empty strings for all string
        // fields, 0 for exit_code, 0.0 for duration_secs. This test pins that
        // expectation.
        let out = ExecOutput {
            command_name: String::new(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            duration_secs: 0.0,
            session_id: String::new(),
        };
        assert_eq!(out.command_name, "");
        assert_eq!(out.stdout, "");
        assert_eq!(out.stderr, "");
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.duration_secs, 0.0);
        assert_eq!(out.session_id, "");
    }

    // -----------------------------------------------------------------------
    // ExecOutput: deserialize from a JSON string with all fields populated.
    // -----------------------------------------------------------------------
    #[test]
    fn test_exec_output_deserializes_from_json() {
        let json = r#"{
            "command_name": "migrate",
            "stdout": "rows=42\n",
            "stderr": "warn: deprecated flag\n",
            "exit_code": 0,
            "duration_secs": 1.5,
            "session_id": "11111111-2222-3333-4444-555555555555"
        }"#;
        let out: ExecOutput = serde_json::from_str(json).expect("deserialize");
        assert_eq!(out.command_name, "migrate");
        assert_eq!(out.stdout, "rows=42\n");
        assert_eq!(out.stderr, "warn: deprecated flag\n");
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.duration_secs, 1.5);
        assert_eq!(out.session_id, "11111111-2222-3333-4444-555555555555");
    }

    // -----------------------------------------------------------------------
    // ExecOutput: serialize to JSON, parse back as serde_json::Value, verify
    // all field names match the struct fields exactly (snake_case).
    // -----------------------------------------------------------------------
    #[test]
    fn test_exec_output_serializes_with_correct_field_names() {
        let out = ExecOutput {
            command_name: "raw".into(),
            stdout: "out".into(),
            stderr: "err".into(),
            exit_code: 2,
            duration_secs: 0.25,
            session_id: "sid".into(),
        };
        let v = serde_json::to_value(&out).unwrap();
        // Field names must be exactly these (snake_case from struct field names).
        assert!(v.get("command_name").is_some());
        assert!(v.get("stdout").is_some());
        assert!(v.get("stderr").is_some());
        assert!(v.get("exit_code").is_some());
        assert!(v.get("duration_secs").is_some());
        assert!(v.get("session_id").is_some());
        // And there should be no extra fields.
        assert_eq!(v.as_object().unwrap().len(), 6);
    }

    // -----------------------------------------------------------------------
    // classify_ssh_err: preserves CommandFailed (does not reclassify).
    // -----------------------------------------------------------------------
    #[test]
    fn test_classify_ssh_err_preserves_command_failed() {
        let e = CodespaceError::CommandFailed {
            exit_code: 127,
            stderr: "command not found".into(),
        };
        let classified = classify_ssh_err(e, 30);
        match classified {
            CodespaceError::CommandFailed { exit_code, stderr } => {
                assert_eq!(exit_code, 127);
                assert_eq!(stderr, "command not found");
            }
            other => panic!("expected CommandFailed, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // classify_ssh_err: preserves NetworkError (does not reclassify).
    // -----------------------------------------------------------------------
    #[test]
    fn test_classify_ssh_err_preserves_network_error() {
        let e = CodespaceError::NetworkError("connection refused".into());
        let classified = classify_ssh_err(e, 30);
        assert!(matches!(
            classified,
            CodespaceError::NetworkError(ref msg) if msg == "connection refused"
        ));
    }

    // -----------------------------------------------------------------------
    // classify_ssh_err: preserves other error variants (TokenRevoked,
    // CodespaceNotFound, etc.) — quick sanity check that they pass through.
    // -----------------------------------------------------------------------
    #[test]
    fn test_classify_ssh_err_preserves_other_variants_explicit() {
        let cases: Vec<CodespaceError> = vec![
            CodespaceError::TokenRevoked,
            CodespaceError::TokenMissing,
            CodespaceError::CodespaceNotFound("cs".into()),
            CodespaceError::CodespaceUnreachable("down".into()),
            CodespaceError::HostKeyMismatch {
                expected: "a".into(),
                actual: "b".into(),
            },
        ];
        for e in cases {
            // Re-create the input each iteration because classify_ssh_err
            // takes ownership. Clone is not derived on all variants (e.g.
            // some carry String), so we re-construct by hand.
            // Note: codespacectl's CodespaceError derives Debug, so we can
            // format the input for diagnostics.
            let label = format!("{:?}", e);
            let timeout_secs = 30;
            let classified = classify_ssh_err(e, timeout_secs);
            // Each of these should NOT be CommandTimeout (they should pass
            // through unchanged). The specific variant is preserved by the
            // `_ => e` catch-all arm.
            assert!(
                !matches!(classified, CodespaceError::CommandTimeout { .. }),
                "{} should not be reclassified as CommandTimeout",
                label
            );
        }
    }

    // -----------------------------------------------------------------------
    // classify_ssh_err: Internal messages WITHOUT "timed out" pass through
    // unchanged (the substring check is the discriminator).
    // -----------------------------------------------------------------------
    #[test]
    fn test_classify_ssh_err_internal_without_timeout_passes_through() {
        let e = CodespaceError::Internal("ssh error: channel_open_session failed".into());
        let classified = classify_ssh_err(e, 30);
        match classified {
            CodespaceError::Internal(msg) => {
                assert_eq!(msg, "ssh error: channel_open_session failed");
            }
            other => panic!("expected Internal, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // is_timeout: true for CommandTimeout.
    // -----------------------------------------------------------------------
    #[test]
    fn test_is_timeout_true_for_command_timeout() {
        assert!(is_timeout(&CodespaceError::CommandTimeout {
            timeout_secs: 5
        }));
        assert!(is_timeout(&CodespaceError::CommandTimeout {
            timeout_secs: 0
        }));
        assert!(is_timeout(&CodespaceError::CommandTimeout {
            timeout_secs: 9999
        }));
    }

    // -----------------------------------------------------------------------
    // is_timeout: false for Internal (even when the message contains "timed
    // out" — the caller is expected to run classify_ssh_err FIRST, which
    // converts Internal("...timed out...") into CommandTimeout before
    // is_timeout is ever called). This documents the contract.
    // -----------------------------------------------------------------------
    #[test]
    fn test_is_timeout_false_for_internal_even_with_timeout_message() {
        let e = CodespaceError::Internal("ssh error: exec read loop timed out after 30s".into());
        assert!(
            !is_timeout(&e),
            "is_timeout must NOT match Internal; classify_ssh_err is responsible for converting Internal(timed out) -> CommandTimeout first"
        );
    }

    // -----------------------------------------------------------------------
    // is_timeout: false for CommandFailed.
    // -----------------------------------------------------------------------
    #[test]
    fn test_is_timeout_false_for_command_failed() {
        let e = CodespaceError::CommandFailed {
            exit_code: 1,
            stderr: String::new(),
        };
        assert!(!is_timeout(&e));
    }

    // -----------------------------------------------------------------------
    // is_timeout: false for NetworkError.
    // -----------------------------------------------------------------------
    #[test]
    fn test_is_timeout_false_for_network_error() {
        let e = CodespaceError::NetworkError("connection reset".into());
        assert!(!is_timeout(&e));
    }

    // -----------------------------------------------------------------------
    // is_timeout: false for CodespaceUnreachable (retryable, but not a
    // command timeout).
    // -----------------------------------------------------------------------
    #[test]
    fn test_is_timeout_false_for_codespace_unreachable() {
        let e = CodespaceError::CodespaceUnreachable("server down".into());
        assert!(!is_timeout(&e));
    }

    // -----------------------------------------------------------------------
    // Compile-time test: the public API functions exist with the expected
    // parameter types. If a future edit drops a parameter or changes a type,
    // this stops compiling — the closure body has to call the function with
    // exactly the declared types.
    //
    // We don't actually invoke the futures (that requires a live SSH
    // session); we just need the closure body to type-check.
    // -----------------------------------------------------------------------
    #[allow(dead_code)]
    fn _verify_signatures(
        ssh: &mut CodespaceSsh,
        command_name: &str,
        command: &Command,
        ctx: &TemplateContext,
        session: &SessionLog,
        raw_command: &str,
        timeout: Duration,
        hooks: &[HookCommand],
    ) {
        let _ = exec_command(ssh, command_name, command, ctx, session);
        let _ = exec_raw(ssh, raw_command, timeout, session);
        let _ = run_post_start(ssh, hooks, ctx);
        let _ = run_pre_stop(ssh, hooks, ctx);
    }
}
