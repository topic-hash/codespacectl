//! Manifest schema — Serde structs matching `CODESPACE.yaml` structure.
//! See `docs/MANIFEST_SPEC.md` for the authoritative spec.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Top-level manifest structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub metadata: Metadata,
    pub environment: Environment,
    #[serde(default)]
    pub commands: HashMap<String, Command>,
    #[serde(default)]
    pub hooks: Option<Hooks>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    #[serde(rename = "workingDir", default = "default_working_dir")]
    pub working_dir: String,
    #[serde(rename = "healthChecks", default)]
    pub health_checks: Vec<HealthCheck>,
    #[serde(default)]
    pub secrets: Vec<Secret>,
}

fn default_working_dir() -> String {
    "/workspaces".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub name: String,
    pub command: String,
    #[serde(rename = "expectExitCode", default = "default_exit_code")]
    pub expect_exit_code: i32,
    #[serde(rename = "timeoutSecs", default = "default_health_timeout")]
    pub timeout_secs: u64,
}

fn default_exit_code() -> i32 {
    0
}

fn default_health_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(rename = "generateIfMissing")]
    pub generate_if_missing: Option<GenerateConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateConfig {
    #[serde(default = "default_secret_length")]
    pub length: u32,
    #[serde(default = "default_charset")]
    pub charset: String,
}

fn default_secret_length() -> u32 {
    24
}

fn default_charset() -> String {
    "alnum+symbols".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    #[serde(default)]
    pub description: Option<String>,
    pub command: String,
    #[serde(rename = "timeoutSecs", default = "default_command_timeout")]
    pub timeout_secs: u64,
    #[serde(rename = "requiresHealth", default)]
    pub requires_health: Vec<String>,
    #[serde(default)]
    pub idempotent: bool,
}

fn default_command_timeout() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hooks {
    #[serde(rename = "postStart", default)]
    pub post_start: Vec<HookCommand>,
    #[serde(rename = "preStop", default)]
    pub pre_stop: Vec<HookCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookCommand {
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(rename = "timeoutSecs", default = "default_command_timeout")]
    pub timeout_secs: u64,
}
