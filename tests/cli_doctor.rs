//! Integration tests — `codespacectl doctor` subcommand.
//!
//! `doctor` runs offline-ish: it probes `python3`/`rustc` on PATH, inspects
//! the token file path, the state file path, the gh binary, and probes
//! `api.github.com` with a 10s timeout. Each check produces an `OK`/`FAIL`
//! line in the human-readable output and a structured entry in `--json`
//! mode.
//!
//! These tests assert on the *shape* of the output (the check names that
//! appear) rather than on specific pass/fail outcomes, since the latter
//! depend on the host environment (network reachability, gh on PATH, …).

mod common;

use predicates::prelude::*;

use common::cargo_bin;

/// `codespacectl doctor` runs and exits 0 or 1 (any single check failing
/// flips the exit code to 1). It must never panic / segfault.
#[test]
fn test_doctor_exit_code_is_0_or_1() {
    let mut cmd = cargo_bin();
    cmd.arg("doctor");
    let assert = cmd.assert();
    let code = assert.get_output().status.code().expect("exit code");
    assert!(
        code == 0 || code == 1,
        "expected doctor exit code 0 or 1, got {}",
        code
    );
}

/// `codespacectl --json doctor` returns a valid JSON envelope with the
/// stable schema marker.
#[test]
fn test_json_doctor_envelope_schema() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(json["schema"], "codespacectl/v1");
}

/// `codespacectl --json doctor` envelope has `ok: true` (the command itself
/// succeeds; the per-check `all_ok` field inside `result` reflects the
/// environment).
#[test]
fn test_json_doctor_envelope_ok() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(json["ok"], true);
}

/// `codespacectl doctor` human-readable output mentions the `python3`
/// check (the first probe in the doctor list).
#[test]
fn test_doctor_output_mentions_python3() {
    let mut cmd = cargo_bin();
    cmd.arg("doctor");
    cmd.assert().stdout(predicate::str::contains("python3"));
}

/// `codespacectl doctor` output mentions the `token` check.
#[test]
fn test_doctor_output_mentions_token() {
    let mut cmd = cargo_bin();
    cmd.arg("doctor");
    cmd.assert()
        .stdout(predicate::str::contains("token").and(predicate::str::contains("token_file")));
}

/// `codespacectl doctor` output mentions the `state_file` check.
#[test]
fn test_doctor_output_mentions_state_file() {
    let mut cmd = cargo_bin();
    cmd.arg("doctor");
    cmd.assert().stdout(predicate::str::contains("state_file"));
}

/// `codespacectl doctor` output mentions the `gh_binary` check.
#[test]
fn test_doctor_output_mentions_gh_binary() {
    let mut cmd = cargo_bin();
    cmd.arg("doctor");
    cmd.assert().stdout(predicate::str::contains("gh_binary"));
}

/// `codespacectl doctor` output mentions the `network` check.
#[test]
fn test_doctor_output_mentions_network() {
    let mut cmd = cargo_bin();
    cmd.arg("doctor");
    cmd.assert().stdout(predicate::str::contains("network"));
}

/// `codespacectl --json doctor` envelope's `result.checks` array contains
/// an entry for each of the 8 doctor checks (python3, rustc, token,
/// token_file, state_file, manifests_registered, gh_binary, network).
#[test]
fn test_json_doctor_result_contains_all_checks() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    let checks = json["result"]["checks"]
        .as_array()
        .expect("result.checks should be an array");
    let names: Vec<String> = checks
        .iter()
        .map(|c| {
            c["name"]
                .as_str()
                .expect("check.name is string")
                .to_string()
        })
        .collect();
    for expected in [
        "python3",
        "rustc",
        "token",
        "token_file",
        "state_file",
        "manifests_registered",
        "gh_binary",
        "network",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "expected doctor checks to include '{}', got {:?}",
            expected,
            names
        );
    }
}
