//! Subcommand handler — `codespacectl health`.
//!
//! Runs the manifest's health checks against a connected codespace and prints
//! the resulting report. Exits 0 if all checks pass ("green"), 1 if any fail
//! ("red").

use crate::cli::{print_envelope, Cli, OutputEnvelope, SessionRef};
use crate::health::{run_all_checks, HealthStatus};
use crate::session::SessionLog;
use crate::ssh::CodespaceSsh;
use crate::state::{load_state, save_state};
use std::time::Duration;

use super::common::{
    authed_client, load_manifest_for, resolve_codespace_name, resolve_gh_bin,
    resolve_template_context,
};

/// Handle the `health` subcommand.
///
/// Resolves the codespace name (from `--codespace` or `state.current_codespace`),
/// validates the token (so we surface a clear error if the PAT is stale),
/// loads the manifest, opens an SSH session, runs all declared health checks,
/// records the result in state, and prints the report.
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    let codespace_arg = match &args.command {
        crate::cli::Commands::Health { codespace } => codespace.clone(),
        _ => unreachable!("dispatch error: health handler called for non-Health command"),
    };
    let codespace = resolve_codespace_name(codespace_arg.as_deref())?;

    // Auth: `authed_client()` resolves the token, constructs the client, and
    // validates the token via the trait. `_client` is unused after this — we
    // keep the call uniform across handlers (per Wave 2 spec) so a stale PAT
    // surfaces as a clean error before we open SSH for health checks.
    let _client = authed_client().await?;

    // Load manifest + resolve secrets + gh binary.
    let (manifest, _manifest_path, manifest_dir) = load_manifest_for(args.manifest.as_deref())?;
    let gh_bin = resolve_gh_bin(Some(&manifest_dir)).await?;

    // Open SSH with a 60s connect timeout (separate from per-check timeout).
    let connect_timeout = Duration::from_secs(60);
    let mut ssh = CodespaceSsh::connect(&codespace, &gh_bin, connect_timeout).await?;

    // Start a session log so this health run is recorded.
    let session = SessionLog::new(&codespace, &manifest.metadata.name)?;
    let session_id = session.id().to_string();
    let session_log_path = session.path().display().to_string();

    // Build the template context (resolves secrets via SecretStore).
    let ctx = resolve_template_context(&manifest)?;

    // Run all declared health checks (empty list → trivially green).
    let checks = &manifest.environment.health_checks;
    let report = run_all_checks(&mut ssh, checks, &ctx).await?;

    // Persist the result in state.codespaces[name].last_health_status.
    {
        let mut state = load_state()?;
        let entry = state.codespaces.entry(codespace.clone()).or_default();
        entry.last_health_status = Some(report.overall.to_string());
        entry.last_health_checked_at = Some(report.checked_at.clone());
        save_state(&state)?;
    }

    // Close SSH.
    ssh.close().await.ok();

    let exit = if report.overall == HealthStatus::Green {
        0
    } else {
        1
    };

    if args.json {
        let env = OutputEnvelope::success_with_session(
            report,
            SessionRef {
                id: session_id,
                log_path: session_log_path,
            },
        );
        print_envelope(env);
    } else {
        println!(
            "Health: {} (codespace '{}', {} checks at {})",
            report.overall,
            codespace,
            report.checks.len(),
            report.checked_at
        );
        for c in &report.checks {
            let mark = if c.passed { "OK  " } else { "FAIL" };
            println!(
                "  {} {:<24} exit={}  {:.2}s",
                mark, c.name, c.exit_code, c.duration_secs
            );
            if !c.stderr.is_empty() {
                for line in c.stderr.lines().take(5) {
                    println!("      {}", line);
                }
            }
        }
        println!("Session log: {}", session_log_path);
    }

    Ok(exit)
}
