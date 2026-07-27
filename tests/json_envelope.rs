//! Integration tests — the JSON output envelope schema.
//!
//! Every `--json` invocation produces a stable envelope with the shape:
//!
//! ```jsonc
//! {
//!   "schema": "codespacectl/v1",
//!   "ok": true,
//!   "result": <T | null>,
//!   "error": null | { kind, message, retryable, suggested_action, context? },
//!   "warnings": ["..."],
//!   "session": null | { id, log_path }
//! }
//! ```
//!
//! These tests use `doctor` (success path) and `discover` with no token
//! (error path) as the canonical exercise of both branches. They assert on
//! the envelope *shape*, not on per-check outcomes (which depend on the
//! host environment).

mod common;

use common::{cargo_bin, temp_config_dir, temp_state_dir};

/// `codespacectl --json doctor` output parses as valid JSON. (We don't
/// assert on the exit code — `doctor` exits 1 if any check fails, but the
/// envelope is still emitted and `ok: true` because the command itself
/// ran successfully.)
#[test]
fn test_json_envelope_parses_as_valid_json() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    serde_json::from_slice::<serde_json::Value>(&output).expect("doctor JSON output should parse");
}

/// The JSON envelope has `schema: "codespacectl/v1"`.
#[test]
fn test_json_envelope_has_schema_field() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["schema"], "codespacectl/v1");
}

/// The JSON envelope has an `ok` field that is a boolean.
#[test]
fn test_json_envelope_has_ok_boolean() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        json["ok"].is_boolean(),
        "ok should be a boolean, got: {:?}",
        json["ok"]
    );
}

/// The JSON envelope has a `result` field that is either an object, an
/// array, or null (but always present).
#[test]
fn test_json_envelope_has_result_field() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        json["result"].is_object() || json["result"].is_array() || json["result"].is_null(),
        "result should be object/array/null, got: {:?}",
        json["result"]
    );
}

/// The JSON envelope has an `error` field that is either null or an object
/// (in the success branch it must be null).
#[test]
fn test_json_envelope_has_error_field() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        json["error"].is_null() || json["error"].is_object(),
        "error should be null or object, got: {:?}",
        json["error"]
    );
}

/// The JSON envelope has a `warnings` field that is an array (empty on
/// the success branch).
#[test]
fn test_json_envelope_has_warnings_array() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        json["warnings"].is_array(),
        "warnings should be an array, got: {:?}",
        json["warnings"]
    );
}

/// The JSON envelope has a `session` field that is either null or an
/// object (null for `doctor` which doesn't open an SSH session).
#[test]
fn test_json_envelope_has_session_field() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "doctor"]);
    let output = cmd.assert().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        json["session"].is_null() || json["session"].is_object(),
        "session should be null or object, got: {:?}",
        json["session"]
    );
}

/// On error (`--json discover` with no token), the envelope has `ok: false`
/// and `error` is an object with the required sub-fields.
#[test]
fn test_json_envelope_error_shape() {
    let (_cfg_tmp, config_home) = temp_config_dir();
    let (_cache_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["--json", "discover"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env_remove("CODESPACECTL_TOKEN");
    let output = cmd.assert().failure().get_output().stdout.clone();
    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("valid JSON envelope on error");

    assert_eq!(json["ok"], false, "ok should be false on error");
    let error = &json["error"];
    assert!(error.is_object(), "error should be an object");

    // Required sub-fields per ErrorEnvelope schema:
    for field in ["kind", "message", "retryable", "suggested_action"] {
        assert!(
            error.get(field).is_some(),
            "error should have field '{}'",
            field
        );
    }
}

/// On error, the `kind` field is a non-empty string.
#[test]
fn test_json_envelope_error_kind_nonempty_string() {
    let (_cfg_tmp, config_home) = temp_config_dir();
    let (_cache_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["--json", "discover"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env_remove("CODESPACECTL_TOKEN");
    let output = cmd.assert().failure().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let kind = json["error"]["kind"]
        .as_str()
        .expect("error.kind should be a string");
    assert!(!kind.is_empty(), "error.kind should be a non-empty string");
}

/// On error, the `retryable` field is a boolean.
#[test]
fn test_json_envelope_error_retryable_boolean() {
    let (_cfg_tmp, config_home) = temp_config_dir();
    let (_cache_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["--json", "discover"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env_remove("CODESPACECTL_TOKEN");
    let output = cmd.assert().failure().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        json["error"]["retryable"].is_boolean(),
        "retryable should be a boolean, got: {:?}",
        json["error"]["retryable"]
    );
}

/// On error, the `suggested_action` field is a non-empty string.
#[test]
fn test_json_envelope_error_suggested_action_nonempty_string() {
    let (_cfg_tmp, config_home) = temp_config_dir();
    let (_cache_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["--json", "discover"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env_remove("CODESPACECTL_TOKEN");
    let output = cmd.assert().failure().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let action = json["error"]["suggested_action"]
        .as_str()
        .expect("suggested_action should be a string");
    assert!(
        !action.is_empty(),
        "suggested_action should be a non-empty string"
    );
}

/// On error, the envelope's `result` field is null and `warnings` is an
/// empty array (errors don't carry partial results).
#[test]
fn test_json_envelope_error_clears_result_and_warnings() {
    let (_cfg_tmp, config_home) = temp_config_dir();
    let (_cache_tmp, cache_home) = temp_state_dir();
    let mut cmd = cargo_bin();
    cmd.args(["--json", "discover"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env_remove("CODESPACECTL_TOKEN");
    let output = cmd.assert().failure().get_output().stdout.clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(json["result"].is_null(), "result should be null on error");
    assert!(
        json["warnings"].is_array() && json["warnings"].as_array().unwrap().is_empty(),
        "warnings should be an empty array on error"
    );
    assert!(json["session"].is_null(), "session should be null on error");
}

/// The schema marker is consistent across success and error envelopes.
#[test]
fn test_json_envelope_schema_consistent_across_branches() {
    // Success branch (doctor).
    let mut cmd_a = cargo_bin();
    cmd_a.args(["--json", "doctor"]);
    let out_a = cmd_a.assert().get_output().stdout.clone();
    let json_a: serde_json::Value = serde_json::from_slice(&out_a).unwrap();

    // Error branch (discover with no token).
    let (_cfg_tmp, config_home) = temp_config_dir();
    let (_cache_tmp, cache_home) = temp_state_dir();
    let mut cmd_b = cargo_bin();
    cmd_b
        .args(["--json", "discover"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env_remove("CODESPACECTL_TOKEN");
    let out_b = cmd_b.assert().failure().get_output().stdout.clone();
    let json_b: serde_json::Value = serde_json::from_slice(&out_b).unwrap();

    assert_eq!(json_a["schema"], json_b["schema"]);
    assert_eq!(json_a["schema"], "codespacectl/v1");
}
