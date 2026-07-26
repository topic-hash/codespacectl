//! Subcommand handler — `codespacectl session log [--last <n> | --session <id>]`.
//!
//! Without `--session`: lists the last N session IDs (most-recent-first).
//! `--session <id>`: dumps every entry in that session's NDJSON log as either
//! formatted text (default) or as a JSON array (under `--json`).

use crate::cli::{Cli, OutputEnvelope, print_envelope};
use crate::cli::args::SessionCommands;
use crate::session::SessionLog;

/// Handle the `session` subcommand (currently only `session log`).
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    let (last, session_id) = match &args.command {
        crate::cli::Commands::Session(SessionCommands::Log { last, session }) => {
            (*last, session.clone())
        }
        _ => unreachable!("dispatch error: session handler called for non-Session command"),
    };

    if let Some(id) = session_id {
        return handle_session_show(args, &id).await;
    }

    // List recent N sessions.
    let recent = SessionLog::list_recent(last)?;
    if args.json {
        let arr: Vec<serde_json::Value> = recent
            .iter()
            .map(|(id, mtime)| {
                serde_json::json!({
                    "id": id,
                    "modified_at": format!("{:?}", mtime),
                })
            })
            .collect();
        let envelope = OutputEnvelope::success(serde_json::json!({
            "sessions": arr,
            "count": arr.len(),
        }));
        print_envelope(envelope);
    } else {
        if recent.is_empty() {
            println!("No session logs found.");
        } else {
            println!("Recent {} session(s):", recent.len());
            for (id, mtime) in &recent {
                println!("  {}  (modified {:?})", id, mtime);
            }
        }
    }
    Ok(0)
}

/// `--session <id>`: print all entries in that session.
async fn handle_session_show(args: &Cli, id: &str) -> crate::Result<i32> {
    let entries = SessionLog::read(id)?;
    if args.json {
        let envelope = OutputEnvelope::success(serde_json::json!({
            "session_id": id,
            "entries": entries,
            "count": entries.len(),
        }));
        print_envelope(envelope);
    } else {
        if entries.is_empty() {
            println!("No entries found for session {}.", id);
            return Ok(0);
        }
        println!("Session {} ({} entries):", id, entries.len());
        for entry in &entries {
            println!("[{}] {:?}  {}", entry.timestamp, entry.kind, entry.data);
        }
    }
    Ok(0)
}
