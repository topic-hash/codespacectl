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

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_YAML: &str = r#"
apiVersion: v1
metadata:
  name: my-app
  description: A test
  repo: topic-hash/DataMigrata
environment:
  workingDir: /workspaces/app
  healthChecks:
    - name: db_ready
      command: pg_isready
      expectExitCode: 0
      timeoutSecs: 30
  secrets:
    - name: SA_PASSWORD
      required: true
      generateIfMissing:
        length: 24
        charset: alnum+symbols
commands:
  migrate:
    description: Run migrations
    command: ./migrate.sh
    timeoutSecs: 600
    requiresHealth:
      - db_ready
    idempotent: true
hooks:
  postStart:
    - command: echo hi
      cwd: /tmp
      timeoutSecs: 10
  preStop:
    - command: echo bye
"#;

    #[test]
    fn test_manifest_deserializes_full_yaml() {
        let m: Manifest = serde_yaml::from_str(FULL_YAML).expect("full manifest should parse");
        assert_eq!(m.api_version, "v1");
        assert_eq!(m.metadata.name, "my-app");
        assert_eq!(m.metadata.description.as_deref(), Some("A test"));
        assert_eq!(m.metadata.repo.as_deref(), Some("topic-hash/DataMigrata"));
        assert_eq!(m.environment.working_dir, "/workspaces/app");
        assert_eq!(m.environment.health_checks.len(), 1);
        assert_eq!(m.environment.secrets.len(), 1);
        assert_eq!(m.commands.len(), 1);
        assert!(m.hooks.is_some());
        let hc = &m.environment.health_checks[0];
        assert_eq!(hc.name, "db_ready");
        assert_eq!(hc.command, "pg_isready");
        assert_eq!(hc.expect_exit_code, 0);
        assert_eq!(hc.timeout_secs, 30);
        let secret = &m.environment.secrets[0];
        assert_eq!(secret.name, "SA_PASSWORD");
        assert!(secret.required);
        let gen = secret.generate_if_missing.as_ref().expect("generate_if_missing");
        assert_eq!(gen.length, 24);
        assert_eq!(gen.charset, "alnum+symbols");
        let migrate = m.commands.get("migrate").expect("migrate command");
        assert_eq!(migrate.command, "./migrate.sh");
        assert_eq!(migrate.timeout_secs, 600);
        assert_eq!(migrate.requires_health, vec!["db_ready".to_string()]);
        assert!(migrate.idempotent);
        let hooks = m.hooks.as_ref().unwrap();
        assert_eq!(hooks.post_start.len(), 1);
        assert_eq!(hooks.post_start[0].command, "echo hi");
        assert_eq!(hooks.post_start[0].cwd.as_deref(), Some("/tmp"));
        assert_eq!(hooks.post_start[0].timeout_secs, 10);
        assert_eq!(hooks.pre_stop.len(), 1);
        assert_eq!(hooks.pre_stop[0].command, "echo bye");
        // cwd defaults to None when absent.
        assert!(hooks.pre_stop[0].cwd.is_none());
        // timeout_secs defaults to 300 (default_command_timeout) when absent.
        assert_eq!(hooks.pre_stop[0].timeout_secs, 300);
    }

    #[test]
    fn test_manifest_deserializes_minimal_yaml() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: minimal
environment:
  workingDir: /workspaces/min
"#;
        let m: Manifest = serde_yaml::from_str(yaml).expect("minimal manifest should parse");
        assert_eq!(m.api_version, "v1");
        assert_eq!(m.metadata.name, "minimal");
        assert!(m.metadata.description.is_none());
        assert!(m.metadata.repo.is_none());
        assert_eq!(m.environment.working_dir, "/workspaces/min");
        assert!(m.environment.health_checks.is_empty());
        assert!(m.environment.secrets.is_empty());
        assert!(m.commands.is_empty());
        assert!(m.hooks.is_none());
    }

    #[test]
    fn test_manifest_commands_defaults_to_empty_hashmap_when_absent() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: no-cmds
environment:
  workingDir: /x
"#;
        let m: Manifest = serde_yaml::from_str(yaml).expect("should parse");
        assert!(m.commands.is_empty(), "commands should default to empty HashMap");
    }

    #[test]
    fn test_manifest_hooks_defaults_to_none_when_absent() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: no-hooks
environment:
  workingDir: /x
"#;
        let m: Manifest = serde_yaml::from_str(yaml).expect("should parse");
        assert!(m.hooks.is_none(), "hooks should default to None when absent");
    }

    #[test]
    fn test_manifest_json_round_trip() {
        let original: Manifest = serde_yaml::from_str(FULL_YAML).expect("parse");
        let json = serde_json::to_string(&original).expect("serialize to JSON");
        let back: Manifest = serde_json::from_str(&json).expect("deserialize from JSON");
        assert_eq!(back.api_version, original.api_version);
        assert_eq!(back.metadata.name, original.metadata.name);
        assert_eq!(back.metadata.description, original.metadata.description);
        assert_eq!(back.metadata.repo, original.metadata.repo);
        assert_eq!(back.environment.working_dir, original.environment.working_dir);
        assert_eq!(
            back.environment.health_checks.len(),
            original.environment.health_checks.len()
        );
        assert_eq!(
            back.environment.health_checks[0].name,
            original.environment.health_checks[0].name
        );
        assert_eq!(back.commands.len(), original.commands.len());
        assert!(back.hooks.is_some());
    }

    #[test]
    fn test_manifest_yaml_round_trip() {
        let original: Manifest = serde_yaml::from_str(FULL_YAML).expect("parse");
        let yaml = serde_yaml::to_string(&original).expect("serialize to YAML");
        let back: Manifest = serde_yaml::from_str(&yaml).expect("deserialize from YAML");
        assert_eq!(back.api_version, original.api_version);
        assert_eq!(back.metadata.name, original.metadata.name);
        assert_eq!(back.environment.working_dir, original.environment.working_dir);
    }

    #[test]
    fn test_health_check_default_exit_code_is_zero() {
        let yaml = "name: x\ncommand: y\n";
        let hc: HealthCheck = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(hc.expect_exit_code, 0);
    }

    #[test]
    fn test_health_check_default_timeout_secs_is_30() {
        let yaml = "name: x\ncommand: y\n";
        let hc: HealthCheck = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(hc.timeout_secs, 30);
    }

    #[test]
    fn test_command_default_timeout_secs_is_300() {
        let yaml = "command: ./run.sh\n";
        let cmd: Command = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(cmd.timeout_secs, 300);
    }

    #[test]
    fn test_command_default_requires_health_is_empty() {
        let yaml = "command: ./run.sh\n";
        let cmd: Command = serde_yaml::from_str(yaml).expect("parse");
        assert!(cmd.requires_health.is_empty());
    }

    #[test]
    fn test_command_default_idempotent_is_false() {
        let yaml = "command: ./run.sh\n";
        let cmd: Command = serde_yaml::from_str(yaml).expect("parse");
        assert!(!cmd.idempotent);
    }

    #[test]
    fn test_command_default_description_is_none() {
        let yaml = "command: ./run.sh\n";
        let cmd: Command = serde_yaml::from_str(yaml).expect("parse");
        assert!(cmd.description.is_none());
    }

    #[test]
    fn test_secret_with_generate_config_parses() {
        let yaml = r#"
name: SA_PASSWORD
required: true
generateIfMissing:
  length: 32
  charset: hex
"#;
        let s: Secret = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(s.name, "SA_PASSWORD");
        assert!(s.required);
        let gen = s.generate_if_missing.expect("generate_if_missing present");
        assert_eq!(gen.length, 32);
        assert_eq!(gen.charset, "hex");
    }

    #[test]
    fn test_secret_without_generate_config_parses() {
        let yaml = "name: API_KEY\nrequired: false\n";
        let s: Secret = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(s.name, "API_KEY");
        assert!(!s.required);
        assert!(s.generate_if_missing.is_none());
    }

    #[test]
    fn test_secret_default_required_is_false() {
        let yaml = "name: X\n";
        let s: Secret = serde_yaml::from_str(yaml).expect("parse");
        assert!(!s.required, "required should default to false");
    }

    #[test]
    fn test_generate_config_default_length_is_24() {
        let yaml = "charset: alnum\n";
        let g: GenerateConfig = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(g.length, 24);
    }

    #[test]
    fn test_generate_config_default_charset_is_alnum_plus_symbols() {
        let yaml = "length: 16\n";
        let g: GenerateConfig = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(g.charset, "alnum+symbols");
    }

    #[test]
    fn test_generate_config_empty_yaml_uses_all_defaults() {
        let g: GenerateConfig = serde_yaml::from_str("").expect("parse empty");
        assert_eq!(g.length, 24);
        assert_eq!(g.charset, "alnum+symbols");
    }

    #[test]
    fn test_environment_default_working_dir_is_workspaces() {
        // When environment block is present but workingDir is absent, the
        // default_working_dir() function kicks in.
        let yaml = r#"
apiVersion: v1
metadata:
  name: x
environment: {}
"#;
        let m: Manifest = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(m.environment.working_dir, "/workspaces");
    }

    #[test]
    fn test_hooks_default_post_start_and_pre_stop_are_empty() {
        let yaml = "postStart: []\n";
        let h: Hooks = serde_yaml::from_str(yaml).expect("parse");
        assert!(h.post_start.is_empty());
        assert!(h.pre_stop.is_empty());
    }

    #[test]
    fn test_hook_command_default_cwd_is_none() {
        let yaml = "command: echo hi\n";
        let hc: HookCommand = serde_yaml::from_str(yaml).expect("parse");
        assert!(hc.cwd.is_none());
    }

    #[test]
    fn test_hook_command_default_timeout_is_300() {
        let yaml = "command: echo hi\n";
        let hc: HookCommand = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(hc.timeout_secs, 300);
    }

    #[test]
    fn test_manifest_metadata_round_trip() {
        let m = Metadata {
            name: "x".into(),
            description: Some("d".into()),
            repo: Some("r".into()),
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let back: Metadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name, "x");
        assert_eq!(back.description.as_deref(), Some("d"));
        assert_eq!(back.repo.as_deref(), Some("r"));
    }

    #[test]
    fn test_environment_serializes_with_camel_case_keys() {
        let env = Environment {
            working_dir: "/x".into(),
            health_checks: vec![],
            secrets: vec![],
        };
        let yaml = serde_yaml::to_string(&env).expect("serialize");
        // serde_yaml should produce `workingDir:` (camelCase) not `working_dir:`.
        assert!(
            yaml.contains("workingDir:"),
            "expected camelCase workingDir in serialized YAML, got: {}",
            yaml
        );
        assert!(
            yaml.contains("healthChecks:"),
            "expected camelCase healthChecks in serialized YAML, got: {}",
            yaml
        );
    }

    #[test]
    fn test_manifest_api_version_serializes_as_api_version() {
        let m = Manifest {
            api_version: "v1".into(),
            metadata: Metadata {
                name: "x".into(),
                description: None,
                repo: None,
            },
            environment: Environment {
                working_dir: "/x".into(),
                health_checks: vec![],
                secrets: vec![],
            },
            commands: HashMap::new(),
            hooks: None,
        };
        let yaml = serde_yaml::to_string(&m).expect("serialize");
        assert!(
            yaml.contains("apiVersion:"),
            "expected camelCase apiVersion in serialized YAML, got: {}",
            yaml
        );
    }
}
