//! Subcommand handler — `codespacectl stop`.
//!
//! Runs the manifest's `preStop` hooks (unless `--skip-hooks`), then calls
//! the GitHub API to stop the codespace. Updates the state file with the new
//! `last_known_state = "Shutdown"`.

use crate::cli::{Cli, OutputEnvelope, print_envelope};
use crate::exec::run_pre_stop;
use crate::manifest::Manifest;
use crate::ssh::CodespaceSsh;
use crate::state::{load_state, save_state};
use std::time::Duration;

use super::common::{authed_client, load_manifest_for, resolve_gh_bin, resolve_template_context};

/// Handle the `stop` subcommand.
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    let (codespace_arg, skip_hooks) = match &args.command {
        crate::cli::Commands::Stop { codespace, skip_hooks } => {
            (codespace.clone(), *skip_hooks)
        }
        _ => unreachable!("dispatch error: stop handler called for non-Stop command"),
    };
    let codespace = super::common::resolve_codespace_name(codespace_arg.as_deref())?;

    // Auth.
    let client = authed_client().await?;

    // Load manifest (for preStop hooks + manifest_dir for gh_bin lookup).
    let (manifest, _manifest_path, manifest_dir) = load_manifest_for(args.manifest.as_deref())?;

    // Run preStop hooks (unless --skip-hooks).
    let hooks_ran = if !skip_hooks {
        run_pre_stop_hooks(&codespace, &manifest, &manifest_dir).await?
    } else {
        0
    };

    // Stop the codespace via the GitHub API.
    client.stop_codespace(&codespace).await?;

    // Update state.
    {
        let mut state = load_state()?;
        let cs_state = state.codespaces.entry(codespace.clone()).or_default();
        cs_state.last_known_state = Some("Shutdown".to_string());
        cs_state.last_checked_at = Some(chrono::Utc::now().to_rfc3339());
        save_state(&state)?;
    }

    if args.json {
        let envelope = OutputEnvelope::success(serde_json::json!({
            "codespace": codespace,
            "state": "Shutdown",
            "hooks_ran": hooks_ran,
        }));
        print_envelope(envelope);
    } else {
        println!(
            "Stopped codespace '{}' (preStop hooks run: {})",
            codespace, hooks_ran
        );
    }
    Ok(0)
}

/// Open an SSH session, run the manifest's `preStop` hooks, and close it.
/// Returns the number of hooks executed (0 if no hooks are declared).
async fn run_pre_stop_hooks(
    codespace: &str,
    manifest: &Manifest,
    manifest_dir: &std::path::Path,
) -> crate::Result<usize> {
    let hooks = match &manifest.hooks {
        Some(h) => &h.pre_stop,
        None => return Ok(0),
    };
    let n = hooks.len();
    if n == 0 {
        return Ok(0);
    }

    let gh_bin = resolve_gh_bin(Some(manifest_dir))?;
    let connect_timeout = Duration::from_secs(60);
    let mut ssh = CodespaceSsh::connect(codespace, &gh_bin, connect_timeout).await?;
    let ctx = resolve_template_context(manifest)?;
    run_pre_stop(&mut ssh, hooks, &ctx).await?;
    ssh.close().await.ok();
    Ok(n)
}
