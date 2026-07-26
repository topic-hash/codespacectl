//! Subcommand handler — `codespacectl connect`.
//!
//! Brings a codespace to `Available`, establishes an SSH session over
//! `gh cs ssh --stdio`, performs TOFU host-key verification, runs `postStart`
//! hooks (unless `--skip-hooks`), and runs manifest health checks (unless
//! `--skip-health`). The SSH session is closed at the end (subsequent
//! `exec`/`raw`/`health` commands open a fresh one).

use crate::cli::{Cli, OutputEnvelope, SessionRef, print_envelope};
use crate::exec::run_post_start;
use crate::health::{run_all_checks, HealthReport};
use crate::manifest::{manifest_sha256, Manifest};
use crate::session::SessionLog;
use crate::ssh::host_keys::{self, HostKeyDecision};
use crate::ssh::CodespaceSsh;
use crate::state::{load_state, save_state};
use serde::Serialize;
use std::time::Duration;

use super::common::{
    authed_client, load_manifest_for, resolve_gh_bin, resolve_template_context,
};

/// JSON-serializable result for the `connect` command.
#[derive(Debug, Serialize)]
struct ConnectResult {
    codespace: String,
    state: String,
    manifest: String,
    sha256: String,
    host_key_fingerprint: Option<String>,
    host_key_decision: String,
    hooks_ran: usize,
    health: Option<HealthReport>,
}

/// Handle the `connect` subcommand.
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    // Extract subcommand args.
    let (codespace, accept_new, skip_health, skip_hooks, timeout) = match &args.command {
        crate::cli::Commands::Connect {
            codespace,
            accept_new_host_key,
            skip_health,
            skip_hooks,
            timeout,
        } => (
            codespace.clone(),
            *accept_new_host_key,
            *skip_health,
            *skip_hooks,
            *timeout,
        ),
        _ => unreachable!("dispatch error: connect handler called for non-Connect command"),
    };

    // 1. Authenticate.
    let client = authed_client().await?;

    // 2. Ensure codespace is running (starts if Shutdown, waits for Available).
    let info = client.ensure_running(&codespace, timeout).await?;

    // 3. Load manifest (from --manifest or auto-discovered) and compute SHA.
    let (manifest, manifest_path, manifest_dir) = load_manifest_for(args.manifest.as_deref())?;
    let manifest_content = std::fs::read_to_string(&manifest_path)?;
    let sha = manifest_sha256(&manifest_content);

    // 4. Update state with current codespace / manifest pointers + codespace info.
    {
        let mut state = load_state()?;
        state.current_codespace = Some(codespace.clone());
        state.current_manifest = Some(manifest_path.display().to_string());
        state.current_manifest_sha256 = Some(sha.clone());
        let cs_state = state.codespaces.entry(codespace.clone()).or_default();
        cs_state.last_known_state = Some(info.state.to_string());
        cs_state.last_checked_at = Some(chrono::Utc::now().to_rfc3339());
        cs_state.created_at = Some(info.created_at.clone());
        save_state(&state)?;
    }

    // 5. Resolve the gh binary path (env var, tools/bin/gh, or PATH).
    let gh_bin = resolve_gh_bin(Some(&manifest_dir))?;

    // 6. Establish the SSH session. Use the user's --timeout but at least 30s
    //    so a tight --timeout 60 doesn't preempt the SSH handshake prematurely.
    let connect_timeout = Duration::from_secs(timeout.max(30));
    let mut ssh = CodespaceSsh::connect(&codespace, &gh_bin, connect_timeout).await?;

    // 7. TOFU host-key verification.
    let incoming_fp: Option<String> = ssh.host_key_fingerprint.clone();
    let mut host_key_decision_str = "unknown".to_string();
    if let Some(fp) = &incoming_fp {
        let cs_state_ref = {
            let st = load_state()?;
            st.codespaces
                .get(&codespace)
                .cloned()
                .unwrap_or_default()
        };
        let decision = host_keys::decide(fp, &cs_state_ref, accept_new);
        host_key_decision_str = match &decision {
            HostKeyDecision::StoreNew => "store_new".to_string(),
            HostKeyDecision::Match => "match".to_string(),
            HostKeyDecision::Rotate { .. } => "rotate".to_string(),
            HostKeyDecision::Reject { .. } => "reject".to_string(),
        };
        let should_store = matches!(
            &decision,
            HostKeyDecision::StoreNew | HostKeyDecision::Rotate { .. }
        );
        // enforce_decision rejects Rotate-without-accept_rotation and Reject.
        match host_keys::enforce_decision(decision, accept_new) {
            Ok(_) => {}
            Err(e) => {
                ssh.close().await.ok();
                return Err(e);
            }
        }
        if should_store {
            let mut st = load_state()?;
            let e = st.codespaces.entry(codespace.clone()).or_default();
            e.host_key_fingerprint = Some(fp.clone());
            e.host_key_stored_at = Some(chrono::Utc::now().to_rfc3339());
            save_state(&st)?;
        }
    }

    // 8. Start a session log (best-effort — append failures don't fail connect).
    let session = SessionLog::new(&codespace, &manifest.metadata.name)?;
    let session_id = session.id().to_string();
    let session_log_path = session.path().display().to_string();

    // 9. Build the template context (resolves secrets via SecretStore).
    let ctx = resolve_template_context(&manifest)?;

    // 10. Run postStart hooks (unless --skip-hooks).
    let hooks_ran = if !skip_hooks {
        run_post_start_hooks(&mut ssh, &manifest, &ctx).await?
    } else {
        0
    };

    // 11. Run health checks (unless --skip-health). Store result in state.
    let health_report = if !skip_health {
        run_health_checks(&mut ssh, &manifest, &ctx, &codespace).await?
    } else {
        None
    };

    // 12. Close the SSH session — subsequent commands open their own.
    ssh.close().await.ok();

    // 13. Emit output.
    let result = ConnectResult {
        codespace: codespace.clone(),
        state: info.state.to_string(),
        manifest: manifest_path.display().to_string(),
        sha256: sha.clone(),
        host_key_fingerprint: incoming_fp,
        host_key_decision: host_key_decision_str,
        hooks_ran,
        health: health_report,
    };

    if args.json {
        let env = OutputEnvelope::success_with_session(
            result,
            SessionRef {
                id: session_id,
                log_path: session_log_path,
            },
        );
        print_envelope(env);
    } else {
        println!("Connected to codespace '{}'", result.codespace);
        println!("  State:             {}", result.state);
        println!("  Manifest:          {}", result.manifest);
        println!("  SHA-256:           {}", result.sha256);
        if let Some(fp) = &result.host_key_fingerprint {
            println!("  Host key:          {} ({})", fp, result.host_key_decision);
        } else {
            println!("  Host key:          (not captured)");
        }
        println!("  postStart hooks:   {}", result.hooks_ran);
        if let Some(hr) = &result.health {
            println!(
                "  Health:            {} ({} checks at {})",
                hr.overall,
                hr.checks.len(),
                hr.checked_at
            );
        } else {
            println!("  Health:            (skipped)");
        }
        println!("  Session log:       {}", session_log_path);
    }

    Ok(0)
}

/// Run the manifest's `postStart` hooks (if any). Returns the number of hooks
/// executed (0 if there are none, or if `hooks` is `None`).
async fn run_post_start_hooks(
    ssh: &mut CodespaceSsh,
    manifest: &Manifest,
    ctx: &crate::manifest::TemplateContext,
) -> crate::Result<usize> {
    if let Some(hooks) = &manifest.hooks {
        let n = hooks.post_start.len();
        if n > 0 {
            run_post_start(ssh, &hooks.post_start, ctx).await?;
        }
        Ok(n)
    } else {
        Ok(0)
    }
}

/// Run the manifest's health checks (if any), then record the result in
/// `state.codespaces[codespace].last_health_status`. Returns `None` if there
/// are no health checks declared.
async fn run_health_checks(
    ssh: &mut CodespaceSsh,
    manifest: &Manifest,
    ctx: &crate::manifest::TemplateContext,
    codespace: &str,
) -> crate::Result<Option<HealthReport>> {
    let checks = &manifest.environment.health_checks;
    if checks.is_empty() {
        return Ok(None);
    }
    let report = run_all_checks(ssh, checks, ctx).await?;
    let mut state = load_state()?;
    let entry = state.codespaces.entry(codespace.to_string()).or_default();
    entry.last_health_status = Some(report.overall.to_string());
    entry.last_health_checked_at = Some(report.checked_at.clone());
    save_state(&state)?;
    Ok(Some(report))
}
