//! Subcommand handler — `codespacectl discover`.
//!
//! Lists all codespaces for the authenticated user via the GitHub Codespaces
//! API and prints them as a table (or JSON array when `--json` is set).

use crate::cli::{Cli, OutputEnvelope, print_envelope};
use crate::state::{load_state, save_state};

use super::common::authed_client;

/// Handle the `discover` subcommand.
///
/// Resolves the token, validates it, lists codespaces via the GitHub API, and
/// updates `state.codespaces[name]` with the latest `last_known_state`,
/// `last_checked_at`, and `created_at` (best-effort — state save errors are
/// propagated, but the codespace table is still printed).
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    let client = authed_client().await?;
    let codespaces = client.list_codespaces().await?;

    // Update per-codespace state with the latest info from the API.
    let mut state = load_state()?;
    let now = chrono::Utc::now().to_rfc3339();
    for cs in &codespaces {
        let entry = state.codespaces.entry(cs.name.clone()).or_default();
        entry.last_known_state = Some(cs.state.to_string());
        entry.last_checked_at = Some(now.clone());
        entry.created_at = Some(cs.created_at.clone());
    }
    save_state(&state)?;

    if args.json {
        let envelope = OutputEnvelope::success(codespaces);
        print_envelope(envelope);
    } else {
        if codespaces.is_empty() {
            println!("No codespaces found.");
        } else {
            println!(
                "{:<42} {:<12} {:<32} {:<25}",
                "NAME", "STATE", "REPO", "CREATED"
            );
            for cs in &codespaces {
                println!(
                    "{:<42} {:<12} {:<32} {:<25}",
                    cs.name,
                    cs.state.to_string(),
                    cs.repository.full_name,
                    cs.created_at
                );
            }
        }
    }
    Ok(0)
}
