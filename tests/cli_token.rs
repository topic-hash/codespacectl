//! Integration tests — `codespacectl token` subcommands.
//!
//! `token get`: prints the token file path (NEVER the token itself).
//! `token set`: reads a token from stdin and writes it (0600 perms on Unix)
//!              to `$XDG_CONFIG_HOME/codespacectl/token`.
//! `token clear`: deletes the token file (no-op if absent).
//!
//! Each test runs in a per-test temp `XDG_CONFIG_HOME` so the real user's
//! token file is never touched.

mod common;

use predicates::prelude::*;

use common::{cargo_bin, temp_config_dir};

/// `codespacectl token get` exits 0 (it just prints the path — no token
/// needed, no network).
#[test]
fn test_token_get_exits_zero() {
    let (_tmp, config_home) = temp_config_dir();
    let mut cmd = cargo_bin();
    cmd.args(["token", "get"]).env("XDG_CONFIG_HOME", &config_home);
    cmd.assert().success();
}

/// `codespacectl --json token get` returns a valid JSON envelope with the
/// token file path under `result.path`.
#[test]
fn test_json_token_get_returns_path() {
    let (_tmp, config_home) = temp_config_dir();
    let mut cmd = cargo_bin();
    cmd.args(["--json", "token", "get"])
        .env("XDG_CONFIG_HOME", &config_home);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["schema"], "codespacectl/v1");
    assert_eq!(json["ok"], true);
    assert!(json["result"]["path"].is_string());
    let path = json["result"]["path"].as_str().unwrap();
    assert!(
        path.ends_with("codespacectl/token") || path.ends_with("codespacectl\\token"),
        "expected token path to end with codespacectl/token, got {}",
        path
    );
}

/// `codespacectl token clear` exits 0 even when no token file exists.
#[test]
fn test_token_clear_no_op_exits_zero() {
    let (_tmp, config_home) = temp_config_dir();
    let mut cmd = cargo_bin();
    cmd.args(["token", "clear"])
        .env("XDG_CONFIG_HOME", &config_home);
    cmd.assert().success();
}

/// `codespacectl token set` reads the token from stdin (piped in via
/// `.write_stdin`) and exits 0 on success.
#[test]
fn test_token_set_reads_stdin() {
    let (_tmp, config_home) = temp_config_dir();
    let mut cmd = cargo_bin();
    cmd.args(["token", "set"])
        .env("XDG_CONFIG_HOME", &config_home)
        .write_stdin("ghp_test_token_value_12345\n");
    cmd.assert().success();
}

/// `codespacectl token set` creates the token file with 0600 perms on Unix.
#[test]
fn test_token_set_creates_file_with_0600_perms() {
    let (_tmp, config_home) = temp_config_dir();
    let mut cmd = cargo_bin();
    cmd.args(["token", "set"])
        .env("XDG_CONFIG_HOME", &config_home)
        .write_stdin("ghp_test_token_value_12345\n");
    cmd.assert().success();

    let token_path =
        std::path::Path::new(&config_home).join("codespacectl").join("token");
    let metadata = std::fs::metadata(&token_path)
        .expect("token file should exist after token set");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        // 0600 == owner read/write only; mask off file type bits.
        let perm_bits = mode & 0o777;
        assert_eq!(
            perm_bits, 0o600,
            "expected token file perms 0600, got {:o}",
            perm_bits
        );
    }
    #[cfg(not(unix))]
    {
        let _ = metadata; // satisfy unused warning on non-Unix
    }
}

/// `codespacectl token set` then `codespacectl token get` shows the path
/// (and reports the file as existing).
#[test]
fn test_token_set_then_get() {
    let (_tmp, config_home) = temp_config_dir();

    // set
    let mut set_cmd = cargo_bin();
    set_cmd
        .args(["token", "set"])
        .env("XDG_CONFIG_HOME", &config_home)
        .write_stdin("ghp_test_token_value_12345\n");
    set_cmd.assert().success();

    // get (human-readable form)
    let mut get_cmd = cargo_bin();
    get_cmd
        .args(["token", "get"])
        .env("XDG_CONFIG_HOME", &config_home);
    get_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("Token file:"))
        .stdout(predicate::str::contains("token is stored"));
}

/// `codespacectl token clear` removes the token file (set then clear then
/// verify the file is gone from disk).
#[test]
fn test_token_clear_removes_file() {
    let (_tmp, config_home) = temp_config_dir();
    let token_path =
        std::path::Path::new(&config_home).join("codespacectl").join("token");

    // set
    let mut set_cmd = cargo_bin();
    set_cmd
        .args(["token", "set"])
        .env("XDG_CONFIG_HOME", &config_home)
        .write_stdin("ghp_test_token_value_12345\n");
    set_cmd.assert().success();
    assert!(token_path.exists(), "token file should exist after set");

    // clear
    let mut clear_cmd = cargo_bin();
    clear_cmd
        .args(["token", "clear"])
        .env("XDG_CONFIG_HOME", &config_home);
    clear_cmd.assert().success();
    assert!(
        !token_path.exists(),
        "token file should be gone after clear"
    );
}

/// `codespacectl token set` → `token clear` → `token get` still works
/// (i.e. `token get` is robust to a missing file).
#[test]
fn test_token_set_clear_get_cycle() {
    let (_tmp, config_home) = temp_config_dir();

    // set
    let mut set_cmd = cargo_bin();
    set_cmd
        .args(["token", "set"])
        .env("XDG_CONFIG_HOME", &config_home)
        .write_stdin("ghp_test_token_value_12345\n");
    set_cmd.assert().success();

    // clear
    let mut clear_cmd = cargo_bin();
    clear_cmd
        .args(["token", "clear"])
        .env("XDG_CONFIG_HOME", &config_home);
    clear_cmd.assert().success();

    // get — should still succeed, and report no token file.
    let mut get_cmd = cargo_bin();
    get_cmd
        .args(["token", "get"])
        .env("XDG_CONFIG_HOME", &config_home);
    get_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("Token file:"))
        .stdout(predicate::str::contains("no token file"));
}

/// `codespacectl --json token set` returns a success envelope with the
/// saved path.
#[test]
fn test_json_token_set_envelope() {
    let (_tmp, config_home) = temp_config_dir();
    let mut cmd = cargo_bin();
    cmd.args(["--json", "token", "set"])
        .env("XDG_CONFIG_HOME", &config_home)
        .write_stdin("ghp_test_token_value_12345\n");
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["schema"], "codespacectl/v1");
    assert_eq!(json["ok"], true);
    assert_eq!(json["result"]["saved"], true);
    assert!(json["result"]["path"].is_string());
}

/// `codespacectl --json token clear` returns a success envelope. After
/// setting, `cleared` should be `true`.
#[test]
fn test_json_token_clear_envelope_after_set() {
    let (_tmp, config_home) = temp_config_dir();

    // set first so clear actually had something to remove.
    let mut set_cmd = cargo_bin();
    set_cmd
        .args(["token", "set"])
        .env("XDG_CONFIG_HOME", &config_home)
        .write_stdin("ghp_test_token_value_12345\n");
    set_cmd.assert().success();

    let mut clear_cmd = cargo_bin();
    clear_cmd
        .args(["--json", "token", "clear"])
        .env("XDG_CONFIG_HOME", &config_home);
    let output = clear_cmd
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["result"]["cleared"], true);
}
