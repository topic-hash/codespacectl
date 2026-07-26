//! Subcommand handler — `codespacectl discover [--repo <r>] [--state <s>]`.
//!
//! Lists all codespaces for the authenticated user via the GitHub Codespaces API.
//! Supports optional filtering by repo or state. With `--json`, returns a stable
//! array schema suitable for programmatic selection (use with `switch`).

use crate::cli::args::*;
use crate::cli::{OutputEnvelope, print_envelope};
use crate::github::{GitHubClient, auth};
use crate::state;
use crate::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct DiscoverEntry {
    index: usize,                  // 1-indexed for human display + `switch --index`
    name: String,
    display_name: Option<String>,
    state: String,
    repository: String,
    created_at: String,
    last_used_at: Option<String>,
    is_current: bool,
}

pub async fn handle(args: &Cli, repo_filter: &Option<String>, state_filter: &Option<String>) -> Result<i32> {
    let token = auth::resolve_token()?;
    let client = GitHubClient::new(token)?;
    let _ = client.validate_token().await?;
    let codespaces = client.list_codespaces().await?;

    // Load state to know which one is "current"
    let state = state::load_state().unwrap_or_default();
    let current = state.current_codespace.as_deref();

    // Apply filters + build entries
    let mut entries: Vec<DiscoverEntry> = Vec::new();
    for (i, cs) in codespaces.into_iter().enumerate() {
        if let Some(repo) = repo_filter {
            if !cs.repository.full_name.contains(repo.as_str()) {
                continue;
            }
        }
        if let Some(want_state) = state_filter {
            if cs.state.to_string() != *want_state {
                continue;
            }
        }
        entries.push(DiscoverEntry {
            index: i + 1,
            name: cs.name.clone(),
            display_name: cs.display_name.clone(),
            state: cs.state.to_string(),
            repository: cs.repository.full_name.clone(),
            created_at: cs.created_at.clone(),
            last_used_at: cs.last_used_at.clone(),
            is_current: Some(cs.name.as_str()) == current,
        });
    }

    if args.json {
        let envelope = OutputEnvelope::success(&entries);
        print_envelope(envelope);
    } else {
        if entries.is_empty() {
            println!("No codespaces match the filter.");
            return Ok(0);
        }
        // Header
        println!(
            "{:<4} {:<40} {:<14} {:<32} {:<24}",
            "#", "NAME", "STATE", "REPO", "CREATED"
        );
        println!("{}", "-".repeat(120));
        for e in &entries {
            let marker = if e.is_current { "*" } else { " " };
            println!(
                "{}{:<3} {:<40} {:<14} {:<32} {:<24}",
                marker, e.index, truncate(&e.name, 40), e.state,
                truncate(&e.repository, 32), truncate(&e.created_at, 24)
            );
        }
        println!();
        println!("(*) = current codespace");
        println!("Switch with: codespacectl switch --index <N>");
        println!("Or:          codespacectl switch --codespace <name>");
    }
    Ok(0)
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n.saturating_sub(3)])
    }
}
