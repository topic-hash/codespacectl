//! Integration tests — `codespacectl state` subcommand.
//!
//! `state` (no flags): prints the state file path + a brief summary.
//! `state --export`: dumps the state file as pretty-printed JSON.
//! `state --import <path>`: replaces the state file with the JSON at `<path>`.
//!
//! Each test runs in a per-test temp `XDG_CACHE_HOME` so the real user's
//! state file is never touched.

mod common;

use predicates::prelude::*;

use common::{cargo_bin, temp_state_dir};

/// `codespacectl state` runs and exits 0 (it returns Ok(0) on every path,
/// even when the state file doesn't exist yet — `load_state` returns
/// `State::default()` in that case).
#[test]
fn test_state_exits_zero() {
    let (_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.arg("state").env("XDG_CACHE_HOME", &cache_home);
    cmd.assert().success();
}

/// `codespacectl --json state` returns a valid JSON envelope with the
/// schema marker.
#[test]
fn test_json_state_envelope() {
    let (_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["--json", "state"])
        .env("XDG_CACHE_HOME", &cache_home);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid JSON envelope");
    assert_eq!(json["schema"], "codespacectl/v1");
    assert_eq!(json["ok"], true);
    assert!(json["result"]["state_file"].is_string());
}

/// `codespacectl state` (no flags) prints the "State file:" header in its
/// human-readable summary.
#[test]
fn test_state_output_mentions_state_file() {
    let (_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.arg("state").env("XDG_CACHE_HOME", &cache_home);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("State file:"));
}

/// `codespacectl state --export` outputs valid JSON (the full State struct
/// pretty-printed).
#[test]
fn test_state_export_outputs_valid_json() {
    let (_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["state", "--export"])
        .env("XDG_CACHE_HOME", &cache_home);
    let output = cmd.assert().success().get_output().stdout.clone();
    let parsed: serde_json::Value =
        serde_json::from_slice(&output).expect("export output is valid JSON");
    // A default-serialized State has these top-level keys.
    assert!(parsed["version"].is_number() || parsed["version"].is_null());
    assert!(parsed["codespaces"].is_object());
    assert!(parsed["manifests"].is_object());
}

/// `codespacectl state --export` output can be parsed as a State struct —
/// i.e. the export shape matches the schema codespacectl expects on import.
/// We verify by re-parsing through `serde_json::Value` and checking the
/// required fields are present and well-typed (rather than depending on
/// the `codespacectl::state::State` type, which is a lib-internal type
/// not re-exported for test use).
#[test]
fn test_state_export_roundtrips_as_state_shape() {
    let (_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["state", "--export"])
        .env("XDG_CACHE_HOME", &cache_home);
    let output = cmd.assert().success().get_output().stdout.clone();
    let parsed: serde_json::Value =
        serde_json::from_slice(&output).expect("export output is valid JSON");
    assert!(parsed["codespaces"].is_object(), "codespaces is object");
    assert!(parsed["manifests"].is_object(), "manifests is object");
}

/// `codespacectl state --import /nonexistent/path` exits non-zero with a
/// sensible error (CodespaceError::Internal — the import handler wraps the
/// fs::read_to_string failure).
#[test]
fn test_state_import_nonexistent_fails() {
    let (_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["state", "--import", "/nonexistent/path/to/import.json"])
        .env("XDG_CACHE_HOME", &cache_home);
    cmd.assert().failure();
}

/// `codespacectl --json state --import /nonexistent` returns an error
/// envelope with `kind: "internal_error"` (the import handler wraps the
/// `fs::read_to_string` failure as `CodespaceError::Internal`).
#[test]
fn test_json_state_import_nonexistent_envelope_kind() {
    let (_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["--json", "state", "--import", "/nonexistent/path/to/import.json"])
        .env("XDG_CACHE_HOME", &cache_home);
    let output = cmd.assert().failure().get_output().stdout.clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid JSON envelope on error");
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["kind"], "internal_error");
}

/// `codespacectl state --import <valid-json>` replaces the state file. We
/// write a valid State JSON to a temp file, import it, then verify the
/// resulting state file at `$XDG_CACHE_HOME/codespacectl/state.json`
/// contains the imported `current_codespace` value.
#[test]
fn test_state_import_replaces_state_file() {
    let (tmp, cache_home) = temp_state_dir();

    // Write a valid State JSON to an import file. The State struct uses
    // serde with `skip_serializing_if = "Option::is_none"` for Option fields
    // and `#[serde(default)]` for maps, so this minimal shape is fine.
    let import_content = serde_json::json!({
        "version": 1,
        "current_codespace": "imported-test-codespace",
        "current_manifest": "/tmp/imported-manifest.yaml",
        "current_manifest_sha256": "abc123",
        "codespaces": {},
        "manifests": {},
    })
    .to_string();
    let import_path = tmp.path().join("import.json");
    std::fs::write(&import_path, &import_content).unwrap();

    let mut cmd = cargo_bin();
    cmd.args([
        "state",
        "--import",
        import_path.to_str().unwrap(),
    ])
    .env("XDG_CACHE_HOME", &cache_home);
    cmd.assert().success();

    // Verify the state file now contains the imported current_codespace.
    let state_file =
        std::path::Path::new(&cache_home).join("codespacectl").join("state.json");
    let content = std::fs::read_to_string(&state_file)
        .expect("state file should exist after import");
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["current_codespace"], "imported-test-codespace");
    assert_eq!(parsed["current_manifest"], "/tmp/imported-manifest.yaml");
    assert_eq!(parsed["current_manifest_sha256"], "abc123");
}

/// `codespacectl --json state --import <valid-json>` returns a success
/// envelope with `result.imported == true`.
#[test]
fn test_json_state_import_success_envelope() {
    let (tmp, cache_home) = temp_state_dir();
    let import_content = serde_json::json!({
        "version": 1,
        "codespaces": {},
        "manifests": {},
    })
    .to_string();
    let import_path = tmp.path().join("import.json");
    std::fs::write(&import_path, &import_content).unwrap();

    let mut cmd = cargo_bin();
    cmd.args([
        "--json",
        "state",
        "--import",
        import_path.to_str().unwrap(),
    ])
    .env("XDG_CACHE_HOME", &cache_home);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["result"]["imported"], true);
}
