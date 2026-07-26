//! Subcommand handler — `codespacectl doctor`.
//!
//! Runs a series of environment checks (python3, rustc, token, state file,
//! registered manifests, gh binary, network reachability to api.github.com)
//! and reports the results. Exits 0 if all checks pass, 1 if any fail.

use crate::cli::{Cli, OutputEnvelope, print_envelope};
use crate::github::auth::{resolve_token, token_file_path};
use crate::state::{load_state, state_file_path};
use serde::Serialize;
use std::process::Command;

/// Result of a single doctor check.
#[derive(Debug, Serialize)]
struct DoctorCheck {
    name: String,
    ok: bool,
    detail: String,
}

/// Handle the `doctor` subcommand.
pub async fn handle(args: &Cli) -> crate::Result<i32> {
    let mut checks: Vec<DoctorCheck> = Vec::new();

    // 1. python3 (used for fallback scripts if any).
    let (py_ok, py_detail) = check_command("python3", &["--version"]);
    checks.push(DoctorCheck {
        name: "python3".into(),
        ok: py_ok,
        detail: py_detail,
    });

    // 2. rustc (we're a Rust binary; verify rustc is on PATH for completeness).
    let (rust_ok, rust_detail) = check_command("rustc", &["--version"]);
    checks.push(DoctorCheck {
        name: "rustc".into(),
        ok: rust_ok,
        detail: rust_detail,
    });

    // 3. Token (env var or token file).
    let token_check = resolve_token();
    let token_ok = token_check.is_ok();
    let token_detail = match token_check {
        Ok(_) => "token resolved (env var or token file)".to_string(),
        Err(e) => e.to_string(),
    };
    checks.push(DoctorCheck {
        name: "token".into(),
        ok: token_ok,
        detail: token_detail,
    });

    // 4. Token file (informational — exists or not).
    let token_path = token_file_path();
    checks.push(DoctorCheck {
        name: "token_file".into(),
        ok: true, // informational; not an error if absent (env var may supply)
        detail: format!("{} (exists: {})", token_path.display(), token_path.exists()),
    });

    // 5. State file.
    let state_path = state_file_path();
    let state_exists = state_path.exists();
    checks.push(DoctorCheck {
        name: "state_file".into(),
        ok: true, // not an error to be missing — created lazily on first write
        detail: if state_exists {
            state_path.display().to_string()
        } else {
            format!("{} (does not exist yet — will be created on first use)", state_path.display())
        },
    });

    // 6. Manifests registered.
    let state = load_state().unwrap_or_default();
    let mc = state.manifests.len();
    checks.push(DoctorCheck {
        name: "manifests_registered".into(),
        ok: mc > 0,
        detail: format!("{} manifest(s) registered", mc),
    });

    // 7. gh binary findable.
    checks.push(check_gh_bin());

    // 8. Network: can we reach api.github.com?
    checks.push(check_network().await);

    let all_ok = checks.iter().all(|c| c.ok);
    let exit = if all_ok { 0 } else { 1 };

    if args.json {
        let envelope = OutputEnvelope::success(serde_json::json!({
            "all_ok": all_ok,
            "checks": checks,
        }));
        print_envelope(envelope);
    } else {
        println!("codespacectl doctor");
        println!("---------------------------");
        for c in &checks {
            let mark = if c.ok { "OK  " } else { "FAIL" };
            println!("{:<5} {:<24} {}", mark, c.name, c.detail);
        }
        println!("---------------------------");
        if all_ok {
            println!("All checks passed.");
        } else {
            let failed = checks.iter().filter(|c| !c.ok).count();
            println!(
                "{} of {} checks failed.",
                failed,
                checks.len()
            );
        }
    }

    Ok(exit)
}

/// Run a binary with the given args, returning (ok, detail) where detail is
/// either the trimmed stdout (on success) or an error message.
fn check_command(bin: &str, args: &[&str]) -> (bool, String) {
    match Command::new(bin).args(args).output() {
        Ok(o) if o.status.success() => (
            true,
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
        ),
        Ok(o) => (
            false,
            format!(
                "{} exited with {}: {}",
                bin,
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            ),
        ),
        Err(e) => (false, format!("{} not on PATH: {}", bin, e)),
    }
}

/// Resolve the gh CLI binary path (mirrors `common::resolve_gh_bin` but
/// returns a `DoctorCheck` instead of a `Result`).
fn check_gh_bin() -> DoctorCheck {
    // Use the same find_gh_binary() helper that connect/exec use, so doctor
    // reflects the actual gh discovery logic (env var, tools/bin/gh, cached
    // download, PATH lookup). Returns the path if found.
    if let Some(path) = crate::github::find_gh_binary() {
        return DoctorCheck {
            name: "gh_binary".into(),
            ok: true,
            detail: path.display().to_string(),
        };
    }
    DoctorCheck {
        name: "gh_binary".into(),
        ok: false,
        detail: "gh not found (set CODESPACECTL_GH_BIN, place at tools/bin/gh, or install gh in PATH)".into(),
    }
}

/// Probe https://api.github.com with a 10s timeout. A 200/401/403 response
/// counts as "network reachable" (401/403 just means we hit GitHub without
/// auth — the network itself is fine).
async fn check_network() -> DoctorCheck {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return DoctorCheck {
                name: "network".into(),
                ok: false,
                detail: format!("HTTP client build failed: {}", e),
            };
        }
    };
    match client.get("https://api.github.com").send().await {
        Ok(resp) => {
            let status = resp.status();
            let reachable = status.is_success() || matches!(status.as_u16(), 401 | 403 | 404);
            DoctorCheck {
                name: "network".into(),
                ok: reachable,
                detail: format!("https://api.github.com -> HTTP {}", status),
            }
        }
        Err(e) => DoctorCheck {
            name: "network".into(),
            ok: false,
            detail: format!("cannot reach api.github.com: {}", e),
        },
    }
}
