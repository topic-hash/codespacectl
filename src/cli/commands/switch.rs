//! Subcommand handler — `codespacectl switch [--codespace <name> | --index <N>]`.
//!
//! Lists codespaces (like `discover` but compact) and lets the user/agent pick one.
//! Without args in a TTY: interactive prompt. Without args in non-TTY (or `--json`):
//! returns the codespace list (same as `discover --json`).
//! With `--codespace <name>`: sets that codespace as current in state (does NOT connect).
//! With `--index <N>`: picks the Nth entry from the discovery list and switches to it.
//!
//! After `switch`, run `codespacectl connect` (without `--codespace`) to connect to
//! the newly selected codespace.

use crate::cli::args::*;
use crate::cli::{OutputEnvelope, print_envelope};
use crate::github::{GitHubClient, auth};
use crate::state::{self, State};
use crate::{CodespaceError, Result};
use serde::Serialize;
use std::io::{self, BufRead, Write};

#[derive(Debug, Serialize)]
struct SwitchResult {
    previous_codespace: Option<String>,
    current_codespace: String,
    state: String,
    repository: String,
    note: String,
}

pub async fn handle(args: &Cli, codespace_arg: &Option<String>, index_arg: &Option<usize>) -> Result<i32> {
    let token = auth::resolve_token()?;
    let client = GitHubClient::new(token)?;
    let _ = client.validate_token().await?;
    let codespaces = client.list_codespaces().await?;

    let mut state = state::load_state().unwrap_or_default();
    let previous = state.current_codespace.clone();

    let chosen_name: String = if let Some(name) = codespace_arg {
        // Verify the codespace exists; allow partial match (first match wins).
        let matched = codespaces
            .iter()
            .find(|c| c.name == *name || c.name.starts_with(name.as_str()))
            .map(|c| c.name.clone())
            .ok_or_else(|| CodespaceError::CodespaceNotFound(name.clone()))?;
        matched
    } else if let Some(idx) = index_arg {
        if *idx == 0 || *idx > codespaces.len() {
            return Err(CodespaceError::CodespaceNotFound(format!(
                "index {} out of range (1..={})",
                idx, codespaces.len()
            )));
        }
        codespaces[idx - 1].name.clone()
    } else {
        // No args — list and pick.
        let chosen = pick_interactively(args, &codespaces, &state, &previous)?;
        match chosen {
            Some(name) => name,
            None => return Ok(0), // user dismissed or --json printed list
        }
    };

    // Look up the codespace's current state + repo
    let info = client.get_codespace(&chosen_name).await?;

    // Update state
    state.current_codespace = Some(chosen_name.clone());
    state::save_state(&state)?;

    let result = SwitchResult {
        previous_codespace: previous,
        current_codespace: chosen_name.clone(),
        state: info.state.to_string(),
        repository: info.repository.full_name.clone(),
        note: "Run `codespacectl connect` to establish SSH session.".to_string(),
    };

    if args.json {
        let envelope = OutputEnvelope::success(&result);
        print_envelope(envelope);
    } else {
        if let Some(prev) = &result.previous_codespace {
            if prev != &result.current_codespace {
                println!("Switched codespace: {} -> {}", prev, result.current_codespace);
            } else {
                println!("Already on: {}", result.current_codespace);
            }
        } else {
            println!("Current codespace set to: {}", result.current_codespace);
        }
        println!("  state:    {}", result.state);
        println!("  repo:     {}", result.repository);
        println!();
        println!("{}", result.note);
    }
    Ok(0)
}

/// Returns Some(name) if the user picked a codespace, or None if we printed the
/// list and the caller should exit (e.g. `--json` mode, or non-TTY with no args).
fn pick_interactively(
    args: &Cli,
    codespaces: &[crate::github::CodespaceInfo],
    state: &State,
    previous: &Option<String>,
) -> Result<Option<String>> {
    if codespaces.is_empty() {
        eprintln!("No codespaces found for the authenticated user.");
        return Ok(None);
    }

    // If --json, print the list as JSON (same as discover --json) and return None.
    if args.json {
        let entries: Vec<serde_json::Value> = codespaces
            .iter()
            .enumerate()
            .map(|(i, cs)| serde_json::json!({
                "index": i + 1,
                "name": cs.name,
                "display_name": cs.display_name,
                "state": cs.state.to_string(),
                "repository": cs.repository.full_name,
                "created_at": cs.created_at,
                "last_used_at": cs.last_used_at,
                "is_current": Some(cs.name.as_str()) == previous.as_deref(),
            }))
            .collect();
        let envelope = OutputEnvelope::success(&entries);
        print_envelope(envelope);
        eprintln!("\nTo switch: codespacectl switch --index <N>  (or --codespace <name>)");
        return Ok(None);
    }

    // Non-TTY (e.g. piped stdin) — print list and exit.
    if !atty_is_terminal(0) {
        println!("{:<4} {:<40} {:<14} {:<32}", "#", "NAME", "STATE", "REPO");
        println!("{}", "-".repeat(95));
        for (i, cs) in codespaces.iter().enumerate() {
            let marker = if Some(cs.name.as_str()) == previous.as_deref() {
                "*"
            } else {
                " "
            };
            println!(
                "{}{:<3} {:<40} {:<14} {:<32}",
                marker, i + 1, truncate(&cs.name, 40), cs.state, truncate(&cs.repository.full_name, 32)
            );
        }
        println!("\n(*) = current. Use `codespacectl switch --codespace <name>` or `--index <N>` to switch.");
        return Ok(None);
    }

    // Interactive TTY — prompt for selection.
    loop {
        println!("Available codespaces:");
        for (i, cs) in codespaces.iter().enumerate() {
            let marker = if Some(cs.name.as_str()) == previous.as_deref() { "*" } else { " " };
            println!("{}[{}] {} ({}, {})", marker, i + 1, cs.name, cs.state, cs.repository.full_name);
        }
        print!("\nEnter number to switch (1-{}) or 'q' to quit: ", codespaces.len());
        io::stdout().flush().ok();

        let mut line = String::new();
        io::stdin().lock().read_line(&mut line).map_err(|e| {
            CodespaceError::Internal(format!("failed to read stdin: {}", e))
        })?;
        let line = line.trim();
        if line.eq_ignore_ascii_case("q") || line.is_empty() {
            return Ok(None);
        }
        match line.parse::<usize>() {
            Ok(n) if n >= 1 && n <= codespaces.len() => {
                return Ok(Some(codespaces[n - 1].name.clone()));
            }
            _ => {
                eprintln!("Invalid input: {}. Please enter a number 1-{}.", line, codespaces.len());
            }
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n.saturating_sub(3)])
    }
}

/// Check if a file descriptor is a TTY. We avoid pulling in the `atty`/`is-terminal`
/// crate by using `libc::isatty` directly (already in dep tree via tokio).
fn atty_is_terminal(fd: i32) -> bool {
    // SAFETY: `isatty` is a simple syscall that reads fd metadata — no UB.
    #[cfg(unix)]
    unsafe {
        libc_isatty(fd) != 0
    }
    #[cfg(not(unix))]
    {
        // On non-Unix, assume not a TTY for safety (forces list-and-exit behavior).
        let _ = fd;
        false
    }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "isatty"]
    fn libc_isatty(fd: i32) -> i32;
}
