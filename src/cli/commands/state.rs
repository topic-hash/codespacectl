//! Subcommand handler — `codespacectl state [--export | --import <path>]`.
//!
//! Without flags: prints the state file path + a brief summary.
//! `--export`: dumps the state file as pretty-printed JSON.
//! `--import <path>`: replaces the state file with the JSON at `<path>`.

use crate::cli::{Cli, OutputEnvelope, print_envelope};
use crate::state::{export_state, import_state, load_state, state_file_path};
use crate::CodespaceError;

/// Handle the `state` subcommand.
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    let (export, import) = match &args.command {
        crate::cli::Commands::State { export, import } => (*export, import.clone()),
        _ => unreachable!("dispatch error: state handler called for non-State command"),
    };

    if export {
        return handle_export(args).await;
    }
    if let Some(path) = import {
        return handle_import(args, &path).await;
    }

    // Default: print state file path + summary.
    let state = load_state()?;
    let path = state_file_path();
    if args.json {
        let envelope = OutputEnvelope::success(serde_json::json!({
            "state_file": path.display().to_string(),
            "current_codespace": state.current_codespace,
            "current_manifest": state.current_manifest,
            "current_manifest_sha256": state.current_manifest_sha256,
            "codespaces_tracked": state.codespaces.len(),
            "manifests_registered": state.manifests.len(),
        }));
        print_envelope(envelope);
    } else {
        println!("State file: {}", path.display());
        match &state.current_codespace {
            Some(cs) => println!("Current codespace:    {}", cs),
            None => println!("Current codespace:    (none — run `codespacectl connect`)"),
        }
        if let Some(m) = &state.current_manifest {
            println!("Current manifest:     {}", m);
        }
        if let Some(sha) = &state.current_manifest_sha256 {
            println!("Manifest SHA-256:     {}", sha);
        }
        println!("Codespaces tracked:   {}", state.codespaces.len());
        println!("Manifests registered: {}", state.manifests.len());
    }
    Ok(0)
}

/// `--export`: print the state file contents as JSON.
async fn handle_export(args: &Cli) -> crate::Result<i32> {
    let content = export_state()?;
    if args.json {
        // Re-parse so we can wrap in the standard envelope.
        let v: serde_json::Value = serde_json::from_str(&content)?;
        let envelope = OutputEnvelope::success(v);
        print_envelope(envelope);
    } else {
        // Print the raw pretty-printed JSON directly.
        println!("{}", content);
    }
    Ok(0)
}

/// `--import <path>`: replace the state file with the JSON at `path`.
async fn handle_import(args: &Cli, path: &str) -> crate::Result<i32> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        CodespaceError::Internal(format!("failed to read import file {}: {}", path, e))
    })?;
    import_state(&content)?;
    if args.json {
        let envelope = OutputEnvelope::success(serde_json::json!({
            "imported": true,
            "path": path,
        }));
        print_envelope(envelope);
    } else {
        println!("Imported state from {}", path);
    }
    Ok(0)
}
