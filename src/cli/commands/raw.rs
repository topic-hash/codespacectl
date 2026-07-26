//! Subcommand handler — `codespacectl raw <command>`.
//!
//! Executes an ad-hoc shell command on the codespace (no manifest lookup
//! needed, no template substitution). The command's exit code is propagated
//! as the process exit code.

use crate::cli::{Cli, OutputEnvelope, SessionRef, print_envelope};
use crate::exec::exec_raw;
use crate::session::SessionLog;
use crate::ssh::CodespaceSsh;
use std::time::Duration;

use super::common::{authed_client, load_manifest_for, resolve_codespace_name, resolve_gh_bin};

/// Handle the `raw` subcommand.
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    let (command, codespace_arg, timeout) = match &args.command {
        crate::cli::Commands::Raw {
            command,
            codespace,
            timeout,
        } => (command.clone(), codespace.clone(), *timeout),
        _ => unreachable!("dispatch error: raw handler called for non-Raw command"),
    };
    let codespace = resolve_codespace_name(codespace_arg.as_deref())?;

    // Auth: `authed_client()` resolves the token, constructs the client, and
    // validates the token via the trait. `_client` is unused after this — we
    // keep the call uniform across handlers (per Wave 2 spec) so a stale PAT
    // surfaces as a clean error before SSH.
    let _client = authed_client().await?;

    // Load manifest (we use it for the session log's manifest_name label and
    // to resolve the manifest dir for tools/bin/gh lookup).
    let (manifest, _manifest_path, manifest_dir) = load_manifest_for(args.manifest.as_deref())?;
    let gh_bin = resolve_gh_bin(Some(&manifest_dir)).await?;

    // Open SSH session.
    let connect_timeout = Duration::from_secs(60);
    let mut ssh = CodespaceSsh::connect(&codespace, &gh_bin, connect_timeout).await?;

    // Session log.
    let session = SessionLog::new(&codespace, &manifest.metadata.name)?;
    let session_id = session.id().to_string();
    let session_log_path = session.path().display().to_string();

    // Execute the raw command (no template substitution).
    let exec_timeout = Duration::from_secs(timeout);
    let output = exec_raw(&mut ssh, &command, exec_timeout, &session).await?;
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
        print!("{}", output.stdout);
        if !output.stderr.is_empty() {
            eprint!("{}", output.stderr);
        }
        eprintln!(
            "[raw exit {} in {:.2}s, session {}]",
            output.exit_code, output.duration_secs, session_id
        );
    }

    Ok(exit_code)
}
