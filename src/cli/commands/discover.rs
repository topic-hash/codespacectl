//! Subcommand handler — `codespacectl discover [--repo <r>] [--state <s>]`.
//!
//! Lists all codespaces for the authenticated user via the GitHub Codespaces API.
//! Supports optional filtering by repo or state. With `--json`, returns a stable
//! array schema suitable for programmatic selection (use with `switch`).

use crate::cli::args::*;
use crate::cli::{print_envelope, OutputEnvelope};
use crate::github::traits::GithubApiClient;
use crate::state;
use crate::Result;
use serde::Serialize;

use super::common::authed_client;

#[derive(Debug, Serialize)]
struct DiscoverEntry {
    index: usize, // 1-indexed for human display + `switch --index`
    name: String,
    display_name: Option<String>,
    state: String,
    repository: String,
    created_at: String,
    last_used_at: Option<String>,
    is_current: bool,
}

pub async fn handle(
    args: &Cli,
    repo_filter: &Option<String>,
    state_filter: &Option<String>,
) -> Result<i32> {
    let client = authed_client().await?;
    let state = state::load_state().unwrap_or_default();
    let current = state.current_codespace.as_deref();
    // Pass `&*client` (a `&dyn GithubApiClient`) into the generic orchestrator.
    // This is the seam Wave 2 introduced: production wires in `GitHubClient`
    // via `authed_client()`, while tests inject `FakeGithubApiClient` directly.
    discover_with(&*client, repo_filter, state_filter, current, args).await
}

/// Use-case orchestrator: list codespaces via the trait, build filterable
/// entries, and print them. Generic over `G` so unit tests can pass a
/// `FakeGithubApiClient` (concrete) while production passes a
/// `&dyn GithubApiClient` (trait object) — `?Sized` permits the latter.
async fn discover_with<G: GithubApiClient + ?Sized>(
    client: &G,
    repo_filter: &Option<String>,
    state_filter: &Option<String>,
    current: Option<&str>,
    args: &Cli,
) -> Result<i32> {
    let codespaces = client.list_codespaces().await?;
    let entries = build_entries(codespaces, repo_filter, state_filter, current);
    print_entries(&entries, args);
    Ok(0)
}

/// Pure filtering + projection: take a `Vec<CodespaceInfo>` plus optional
/// repo/state filters and the "current" codespace name, return the
/// 1-indexed `DiscoverEntry` rows that match. Extracted from `handle`
/// so unit tests can verify filter behavior without touching stdout or
/// the GitHub API.
fn build_entries(
    codespaces: Vec<crate::github::CodespaceInfo>,
    repo_filter: &Option<String>,
    state_filter: &Option<String>,
    current: Option<&str>,
) -> Vec<DiscoverEntry> {
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
    entries
}

/// Print `entries` either as a JSON envelope (when `args.json`) or as a
/// fixed-width table. Empty entries print "No codespaces match the filter."
/// in human mode, or an empty JSON array in `--json` mode.
fn print_entries(entries: &[DiscoverEntry], args: &Cli) {
    if args.json {
        let envelope = OutputEnvelope::success(entries);
        print_envelope(envelope);
        return;
    }
    if entries.is_empty() {
        println!("No codespaces match the filter.");
        return;
    }
    // Header
    println!(
        "{:<4} {:<40} {:<14} {:<32} {:<24}",
        "#", "NAME", "STATE", "REPO", "CREATED"
    );
    println!("{}", "-".repeat(120));
    for e in entries {
        let marker = if e.is_current { "*" } else { " " };
        println!(
            "{}{:<3} {:<40} {:<14} {:<32} {:<24}",
            marker,
            e.index,
            truncate(&e.name, 40),
            e.state,
            truncate(&e.repository, 32),
            truncate(&e.created_at, 24)
        );
    }
    println!();
    println!("(*) = current codespace");
    println!("Switch with: codespacectl switch --index <N>");
    println!("Or:          codespacectl switch --codespace <name>");
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    //! Wave 2 use-case tests for `discover`. These exercise:
    //!   1. The trait-driven orchestrator (`discover_with`) — verifies the
    //!      use case correctly delegates to `GithubApiClient::list_codespaces`
    //!      and tolerates an empty result.
    //!   2. The pure filtering logic (`build_entries`) — verifies repo and
    //!      state substring filters behave as documented, without depending
    //!      on stdout or the GitHub API.
    use super::*;
    use crate::github::codespaces::CodespaceState;
    use crate::github::traits::test_support::{
        make_info, make_info_with_repo, FakeGithubApiClient,
    };

    /// Construct a `Cli` for the `Discover` subcommand. `json` toggles the
    /// output mode so we can exercise both branches of `print_entries`.
    fn test_cli(json: bool) -> Cli {
        Cli {
            json,
            verbose: 0,
            manifest: None,
            command: Commands::Discover {
                repo: None,
                state: None,
            },
        }
    }

    /// `build_entries` with an empty input returns an empty vec — verifies
    /// the filter loop doesn't panic on the degenerate case.
    #[test]
    fn test_build_entries_empty_input_returns_empty() {
        let entries = build_entries(vec![], &None, &None, None);
        assert!(entries.is_empty(), "expected empty entries for empty input");
    }

    /// Two codespaces with different repo full_names; filter by a substring
    /// unique to one — expect exactly one entry, the matching one.
    #[test]
    fn test_build_entries_repo_filter_matches_substring() {
        let cs1 = make_info_with_repo("alpha", CodespaceState::Available, "topic-hash/RepoA");
        let cs2 = make_info_with_repo("beta", CodespaceState::Available, "topic-hash/RepoB");
        let entries = build_entries(vec![cs1, cs2], &Some("RepoA".to_string()), &None, None);
        assert_eq!(entries.len(), 1, "repo filter should match exactly 1 entry");
        assert_eq!(entries[0].name, "alpha");
        assert_eq!(entries[0].repository, "topic-hash/RepoA");
    }

    /// Two codespaces in different states; filter by "Available" — expect
    /// only the Available one survives.
    #[test]
    fn test_build_entries_state_filter_keeps_only_matching_state() {
        let cs1 = make_info("alpha", CodespaceState::Available);
        let cs2 = make_info("beta", CodespaceState::Shutdown);
        let entries = build_entries(vec![cs1, cs2], &None, &Some("Available".to_string()), None);
        assert_eq!(
            entries.len(),
            1,
            "state filter should match exactly 1 entry"
        );
        assert_eq!(entries[0].name, "alpha");
        assert_eq!(entries[0].state, "Available");
    }

    /// `discover_with` against an empty `FakeGithubApiClient` must complete
    /// without error and return exit code 0 — exercises the trait seam
    /// end-to-end (the use case correctly calls `list_codespaces` on the
    /// injected fake rather than constructing a real `GitHubClient`).
    #[tokio::test]
    async fn test_discover_with_empty_codespaces_returns_ok() {
        let fake = FakeGithubApiClient::new(vec![]);
        let cli = test_cli(false);
        let result = discover_with(&fake, &None, &None, None, &cli).await;
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(result.unwrap(), 0);
    }
}
