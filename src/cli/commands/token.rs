//! Subcommand handler — `codespacectl token set | get | clear`.
//!
//! - `set`: read the token from stdin (no echo suppression — operator should
//!   use `codespacectl token set < /path/to/token` to keep it out of the
//!   shell history). Saved to `~/.config/codespacectl/token` with 0600 perms.
//! - `get`: prints the token file path (NEVER the token itself).
//! - `clear`: deletes the token file (no-op if absent).

use crate::cli::args::TokenCommands;
use crate::cli::{print_envelope, Cli, OutputEnvelope};
use crate::github::auth::{clear_token, save_token, token_file_path};
use crate::CodespaceError;
use std::io::Read;

/// Handle the `token` subcommand.
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    let cmd = match &args.command {
        crate::cli::Commands::Token(c) => c,
        _ => unreachable!("dispatch error: token handler called for non-Token command"),
    };
    match cmd {
        TokenCommands::Set => set_token(args).await,
        TokenCommands::Get => get_token(args).await,
        TokenCommands::Clear => clear_token_cmd(args).await,
    }
}

/// `token set` — read token from stdin and save it to the token file.
///
/// Note: we don't disable terminal echo (no `termios` / `rpassword` dep).
/// Print a warning so the operator knows their input is visible; suggest
/// using shell redirection to avoid the warning.
async fn set_token(args: &Cli) -> crate::Result<i32> {
    eprintln!(
        "warning: stdin echo is on. To suppress, redirect from a file: \
         `codespacectl token set < /path/to/token`."
    );
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let token = input.trim().to_string();
    if token.is_empty() {
        return Err(CodespaceError::Internal(
            "token read from stdin is empty — refusing to save an empty token".into(),
        ));
    }
    save_token(&token)?;
    let path = token_file_path();
    if args.json {
        let envelope = OutputEnvelope::success(serde_json::json!({
            "saved": true,
            "path": path.display().to_string(),
        }));
        print_envelope(envelope);
    } else {
        println!("Token saved to {}", path.display());
    }
    Ok(0)
}

/// `token get` — print the token file path (NOT the token itself).
async fn get_token(args: &Cli) -> crate::Result<i32> {
    let path = token_file_path();
    let exists = path.exists();
    if args.json {
        let envelope = OutputEnvelope::success(serde_json::json!({
            "path": path.display().to_string(),
            "exists": exists,
        }));
        print_envelope(envelope);
    } else {
        println!("Token file: {}", path.display());
        if exists {
            println!("(token is stored; contents not displayed for security)");
        } else {
            println!("(no token file — set one with `codespacectl token set`)");
        }
    }
    Ok(0)
}

/// `token clear` — delete the token file (no-op if absent).
async fn clear_token_cmd(args: &Cli) -> crate::Result<i32> {
    let path = token_file_path();
    let existed = path.exists();
    clear_token()?;
    if args.json {
        let envelope = OutputEnvelope::success(serde_json::json!({
            "cleared": existed,
            "path": path.display().to_string(),
        }));
        print_envelope(envelope);
    } else if existed {
        println!("Token file removed: {}", path.display());
    } else {
        println!("No token file to remove (was already absent).");
    }
    Ok(0)
}
