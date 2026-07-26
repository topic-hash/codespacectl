//! Shared helpers for codespacectl integration tests.
//!
//! These helpers wrap the boilerplate of:
//! - building an `assert_cmd::Command` that points at the compiled
//!   `codespacectl` binary
//! - creating a per-test temp directory for `XDG_CACHE_HOME` (state file)
//! - creating a per-test temp directory for `XDG_CONFIG_HOME` (token file)
//! - writing a manifest file to a temp dir
//! - returning sample manifest YAML strings (valid + invalid variants)
//!
//! Test files consume this module by declaring `mod common;` at the top.
//
// Each integration test file pulls in only the helpers it needs, so unused
// helpers will warn under default lint settings. We allow dead_code here
// rather than gating each helper behind per-test-file `cfg` blocks.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

/// Returns a fresh `assert_cmd::Command` pointed at the compiled
/// `codespacectl` binary. Each test should call this to get its own command
/// instance (assert_cmd::Command is single-use).
pub fn cargo_bin() -> Command {
    Command::cargo_bin("codespacectl").expect("codespacectl binary should be built")
}

/// Create a per-test temp directory suitable for `XDG_CACHE_HOME` (where
/// codespacectl writes its `state.json` and cached manifests).
///
/// Returns the `TempDir` (which must be held alive for the duration of the
/// test — dropping it cleans up the directory) and the absolute path as a
/// `String` (suitable for passing to `.env("XDG_CACHE_HOME", ...)`).
pub fn temp_state_dir() -> (TempDir, String) {
    let dir = TempDir::new().expect("failed to create temp state dir");
    let path = dir.path().display().to_string();
    (dir, path)
}

/// Create a per-test temp directory suitable for `XDG_CONFIG_HOME` (where
/// codespacectl writes its token file).
///
/// Same shape as `temp_state_dir()`.
pub fn temp_config_dir() -> (TempDir, String) {
    let dir = TempDir::new().expect("failed to create temp config dir");
    let path = dir.path().display().to_string();
    (dir, path)
}

/// Write a manifest (the provided content) to `<dir>/CODESPACE.yaml` and
/// return the path. Useful for `--manifest <path>` and `init <path>` tests.
pub fn write_test_manifest(dir: &Path, content: &str) -> PathBuf {
    let path = dir.join("CODESPACE.yaml");
    std::fs::write(&path, content).expect("failed to write test manifest");
    path
}

/// A minimal valid CODESPACE.yaml. Has `apiVersion: v1`, a lowercase name,
/// an absolute `workingDir`, and no commands/hooks (optional in the schema).
pub fn valid_manifest_yaml() -> &'static str {
    r#"apiVersion: v1
metadata:
  name: test-codespace
  description: minimal valid manifest for integration tests
environment:
  workingDir: /workspaces/test-codespace
"#
}

/// A manifest that fails schema validation: `metadata.name` is missing.
/// `serde_yaml` will fail to deserialize this (the `name` field is required
/// in `Metadata`), surfacing as `ManifestInvalid` with a YAML parse error.
pub fn invalid_manifest_yaml_missing_name() -> &'static str {
    r#"apiVersion: v1
metadata:
  description: manifest missing the required name field
environment:
  workingDir: /workspaces/test-codespace
"#
}

/// A manifest with a valid YAML structure but an unsupported `apiVersion`.
/// `validate_manifest` rejects anything other than `v1`, surfacing as
/// `ManifestVersionUnsupported`.
pub fn invalid_manifest_yaml_bad_api_version() -> &'static str {
    r#"apiVersion: v2
metadata:
  name: test-codespace
environment:
  workingDir: /workspaces/test-codespace
"#
}

/// A manifest with a valid structure but an invalid `metadata.name`
/// (uppercase letters are not allowed — must match `^[a-z0-9-]+$`).
pub fn invalid_manifest_yaml_invalid_name() -> &'static str {
    r#"apiVersion: v1
metadata:
  name: Invalid_Name
environment:
  workingDir: /workspaces/test-codespace
"#
}

/// A manifest with malformed YAML syntax (unterminated quote). Triggers
/// `ManifestInvalid` via the `From<serde_yaml::Error>` conversion.
pub fn invalid_manifest_yaml_malformed() -> &'static str {
    "apiVersion: v1\nmetadata:\n  name: \"unterminated\n"
}
