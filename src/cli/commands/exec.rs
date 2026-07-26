//! Subcommand handler — `codespacectl exec <command>`.
//!
//! Looks up a manifest-declared command by name, runs a health gate (unless
//! `--force`), opens an SSH session, executes the command, and propagates the
//! command's remote exit code as the process exit code.

use crate::cli::{Cli, OutputEnvelope, SessionRef, print_envelope};
use crate::exec::exec_command;
use crate::github::auth::resolve_token;
use crate::github::GitHubClient;
use crate::health::{run_all_checks, HealthStatus};
use crate::session::SessionLog;
use crate::ssh::CodespaceSsh;
use crate::state::load_state;
use crate::state::save_state;
use crate::CodespaceError;
use std::time::Duration;

use super::common::{
    load_manifest_for, resolve_codespace_name, resolve_gh_bin, resolve_template_context,
};

/// Handle the `exec` subcommand.
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    // Extract subcommand args.
    let (command_name, codespace_arg, force, timeout_override) = match &args.command {
        crate::cli::Commands::Exec {
            command,
            codespace,
            force,
            timeout,
        } => (command.clone(), codespace.clone(), *force, *timeout),
        _ => unreachable!("dispatch error: exec handler called for non-Exec command"),
    };
    let codespace = resolve_codespace_name(codespace_arg.as_deref())?;

    // Auth (validate token so a stale PAT fails fast).
    let token = resolve_token()?;
    let client = GitHubClient::new(token)?;
    client.validate_token().await?;

    // Load manifest and look up the command.
    let (manifest, _manifest_path, manifest_dir) = load_manifest_for(args.manifest.as_deref())?;
    let command = manifest
        .commands
        .get(&command_name)
        .ok_or_else(|| {
            CodespaceError::ManifestInvalid(format!(
                "command '{}' not found in manifest (declared: {})",
                command_name,
                manifest
                    .commands
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?
        .clone();

    // Health gate: unless --force, run health checks if last known state is
    // missing or "red". If the fresh run is still red, abort with HealthCheckFailed.
    if !force {
        run_health_gate(&codespace, &manifest, &manifest_dir).await?;
    }

    // Apply timeout override (if any).
    let mut command = command;
    if let Some(t) = timeout_override {
        command.timeout_secs = t;
    }

    // Open SSH session.
    let gh_bin = resolve_gh_bin(Some(&manifest_dir))?;
    let connect_timeout = Duration::from_secs(60);
    let mut ssh = CodespaceSsh::connect(&codespace, &gh_bin, connect_timeout).await?;

    // Session log.
    let session = SessionLog::new(&codespace, &manifest.metadata.name)?;
    let session_id = session.id().to_string();
    let session_log_path = session.path().display().to_string();

    // Template context (with secrets resolved).
    let ctx = resolve_template_context(&manifest)?;

    // Execute the command (non-zero exit code is NOT an error here — it's
    // returned as Ok(ExecOutput) with exit_code set, and we propagate that
    // exit code as our process exit code).
    let output = exec_command(&mut ssh, &command_name, &command, &ctx, &session).await?;
    let exit_code = output.exit_code;

    // Close SSH.
    ssh.close().await.ok();

    if args.json {
        let env = OutputEnvelope::success_with_session(
            output,
            SessionRef {
                id: session_id,
                log_path: session_log_path,
            },
        );
        print_envelope(env);
    } else {
        // Print the command's stdout/stderr so it's pipable, then a brief
        // summary line on stderr so stdout stays clean.
        print!("{}", output.stdout);
        if !output.stderr.is_empty() {
            eprint!("{}", output.stderr);
        }
        eprintln!(
            "[{} exit {} in {:.2}s, session {}]",
            output.command_name, output.exit_code, output.duration_secs, session_id
        );
    }

    Ok(exit_code)
}

/// Run the health gate for `exec`: if `state.codespaces[codespace].last_health_status`
/// is `None` or `"red"`, run all manifest health checks now. If the fresh run
/// is red, return `HealthCheckFailed`. Otherwise persist the fresh status and
/// return Ok.
async fn run_health_gate(
    codespace: &str,
    manifest: &crate::manifest::Manifest,
    manifest_dir: &std::path::Path,
) -> crate::Result<()> {
    let needs_check = {
        let state = load_state()?;
        match state
            .codespaces
            .get(codespace)
            .and_then(|c| c.last_health_status.as_deref())
        {
            None => true,
            Some("green") => false,
            Some(_) => true, // "red" or anything else
        }
    };
    if !needs_check {
        return Ok(());
    }

    let gh_bin = resolve_gh_bin(Some(manifest_dir))?;
    let connect_timeout = Duration::from_secs(60);
    let mut ssh = CodespaceSsh::connect(codespace, &gh_bin, connect_timeout).await?;
    let ctx = resolve_template_context(manifest)?;
    let report = run_all_checks(&mut ssh, &manifest.environment.health_checks, &ctx).await?;

    // Persist fresh status (even on red, so the next `exec` knows the last
    // known state without re-running).
    {
        let mut st = load_state()?;
        let entry = st.codespaces.entry(codespace.to_string()).or_default();
        entry.last_health_status = Some(report.overall.to_string());
        entry.last_health_checked_at = Some(report.checked_at.clone());
        save_state(&st)?;
    }

    ssh.close().await.ok();

    if report.overall == HealthStatus::Red {
        // Find the first failing check for a useful error message.
        let failed = report
            .checks
            .iter()
            .find(|c| !c.passed)
            .map(|c| (c.name.clone(), c.exit_code, c.stderr.clone()))
            .unwrap_or(("overall".to_string(), -1, String::new()));
        return Err(CodespaceError::HealthCheckFailed {
            check: failed.0,
            exit_code: failed.1,
            stderr: failed.2,
        });
    }

    Ok(())
}
