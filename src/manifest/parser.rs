//! Manifest parser and validator. Filled in by Wave 2 subagent.
//!
//! TODO (subagent): implement `parse_manifest`, `parse_manifest_from_file`,
//! `validate_manifest` per the spec in `docs/MANIFEST_SPEC.md`.

use super::schema::Manifest;
use crate::Result;
use std::path::Path;

/// Parse a manifest from a YAML string.
pub fn parse_manifest(content: &str) -> Result<Manifest> {
    let manifest: Manifest = serde_yaml::from_str(content)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

/// Load and parse a manifest from a file path.
pub fn parse_manifest_from_file(path: &Path) -> Result<Manifest> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        crate::CodespaceError::ManifestInvalid(format!("failed to read {}: {}", path.display(), e))
    })?;
    parse_manifest(&content)
}

/// Validate a parsed manifest against the v1 schema rules.
///
/// Checks:
/// - api_version == "v1"
/// - metadata.name is non-empty and matches `^[a-z0-9-]+$`
/// - environment.working_dir is absolute
/// - All health check names are unique
/// - All secret names are unique
/// - All command names are unique (HashMap enforces this)
/// - All `requiresHealth` references exist in `health_checks`
pub fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.api_version != "v1" {
        return Err(crate::CodespaceError::ManifestVersionUnsupported(
            manifest.api_version.clone(),
        ));
    }

    let name = &manifest.metadata.name;
    if name.is_empty() {
        return Err(crate::CodespaceError::ManifestInvalid(
            "metadata.name must be non-empty".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(crate::CodespaceError::ManifestInvalid(format!(
            "metadata.name must match [a-z0-9-]+, got: {}",
            name
        )));
    }

    let working_dir = &manifest.environment.working_dir;
    if !working_dir.starts_with('/') {
        return Err(crate::CodespaceError::ManifestInvalid(format!(
            "environment.workingDir must be absolute, got: {}",
            working_dir
        )));
    }

    // Check for duplicate health check names
    let mut seen = std::collections::HashSet::new();
    for hc in &manifest.environment.health_checks {
        if !seen.insert(&hc.name) {
            return Err(crate::CodespaceError::ManifestInvalid(format!(
                "duplicate health check name: {}",
                hc.name
            )));
        }
    }

    // Check for duplicate secret names
    let mut seen = std::collections::HashSet::new();
    for s in &manifest.environment.secrets {
        if !seen.insert(&s.name) {
            return Err(crate::CodespaceError::ManifestInvalid(format!(
                "duplicate secret name: {}",
                s.name
            )));
        }
    }

    // Check requiresHealth references
    let health_names: std::collections::HashSet<&str> = manifest
        .environment
        .health_checks
        .iter()
        .map(|hc| hc.name.as_str())
        .collect();

    for (cmd_name, cmd) in &manifest.commands {
        for req in &cmd.requires_health {
            if !health_names.contains(req.as_str()) {
                return Err(crate::CodespaceError::ManifestInvalid(format!(
                    "command '{}' requiresHealth '{}' but no health check with that name exists",
                    cmd_name, req
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::schema::{Command, Environment, HealthCheck, Metadata, Secret};
    use super::*;
    use std::collections::HashMap;

    /// A minimal valid manifest — only the fields the schema requires.
    const MINIMAL_MANIFEST: &str = r#"
apiVersion: v1
metadata:
  name: my-app
environment:
  workingDir: /workspaces/my-app
"#;

    /// A full manifest exercising every field, including commands, hooks,
    /// health checks, secrets, and generate config.
    const FULL_MANIFEST: &str = r#"
apiVersion: v1
metadata:
  name: my-app
  description: A test manifest
  repo: topic-hash/DataMigrata
environment:
  workingDir: /workspaces/DataMigrata
  healthChecks:
    - name: db_ready
      command: pg_isready
      expectExitCode: 0
      timeoutSecs: 15
    - name: web_up
      command: curl -sf http://localhost:8080/health
  secrets:
    - name: SA_PASSWORD
      required: true
      generateIfMissing:
        length: 32
        charset: alnum
    - name: API_KEY
      required: false
commands:
  migrate:
    description: Run database migrations
    command: ./migrate.sh
    timeoutSecs: 600
    requiresHealth:
      - db_ready
    idempotent: true
  seed:
    command: ./seed.sh
hooks:
  postStart:
    - command: echo hi
      timeoutSecs: 10
  preStop:
    - command: echo bye
"#;

    #[test]
    fn test_parse_manifest_accepts_valid_full() {
        let m = parse_manifest(FULL_MANIFEST).expect("full manifest should parse");
        assert_eq!(m.api_version, "v1");
        assert_eq!(m.metadata.name, "my-app");
        assert_eq!(m.metadata.description.as_deref(), Some("A test manifest"));
        assert_eq!(m.metadata.repo.as_deref(), Some("topic-hash/DataMigrata"));
        assert_eq!(m.environment.working_dir, "/workspaces/DataMigrata");
        assert_eq!(m.environment.health_checks.len(), 2);
        assert_eq!(m.environment.secrets.len(), 2);
        assert_eq!(m.commands.len(), 2);
        assert!(m.hooks.is_some());
        let migrate = m.commands.get("migrate").expect("migrate command exists");
        assert_eq!(migrate.command, "./migrate.sh");
        assert_eq!(migrate.timeout_secs, 600);
        assert_eq!(migrate.requires_health, vec!["db_ready".to_string()]);
        assert!(migrate.idempotent);
    }

    #[test]
    fn test_parse_manifest_accepts_minimal() {
        let m = parse_manifest(MINIMAL_MANIFEST).expect("minimal manifest should parse");
        assert_eq!(m.api_version, "v1");
        assert_eq!(m.metadata.name, "my-app");
        assert_eq!(m.environment.working_dir, "/workspaces/my-app");
        assert!(m.environment.health_checks.is_empty());
        assert!(m.environment.secrets.is_empty());
        assert!(m.commands.is_empty());
        assert!(m.hooks.is_none());
    }

    #[test]
    fn test_parse_manifest_rejects_empty_string() {
        let err = parse_manifest("").unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
    }

    #[test]
    fn test_parse_manifest_rejects_non_yaml() {
        let _ = parse_manifest("this is just plain text:\n   :::\n   : ::").unwrap_err();
        // Either YAML parse error (ManifestInvalid) or schema error.
        // `null` parses as an empty document, so use something the parser
        // definitely rejects.
        let err = parse_manifest("not: [unterminated").unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
    }

    #[test]
    fn test_parse_manifest_rejects_unsupported_api_version() {
        let yaml = r#"
apiVersion: v2
metadata:
  name: my-app
environment:
  workingDir: /workspaces/my-app
"#;
        let err = parse_manifest(yaml).unwrap_err();
        assert_eq!(
            err.kind(),
            "manifest_version_unsupported",
            "expected manifest_version_unsupported, got: {} ({})",
            err.kind(),
            err
        );
    }

    #[test]
    fn test_parse_manifest_rejects_missing_metadata_name() {
        // metadata.name field is required by the schema; missing it
        // produces a serde_yaml deserialization error → ManifestInvalid.
        let yaml = r#"
apiVersion: v1
metadata:
  description: no name here
environment:
  workingDir: /workspaces/my-app
"#;
        let err = parse_manifest(yaml).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
    }

    #[test]
    fn test_parse_manifest_rejects_uppercase_name() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: My-App
environment:
  workingDir: /workspaces/my-app
"#;
        let err = parse_manifest(yaml).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
        assert!(err.to_string().to_lowercase().contains("name"));
    }

    #[test]
    fn test_parse_manifest_rejects_special_chars_in_name() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: my_app!
environment:
  workingDir: /workspaces/my-app
"#;
        let err = parse_manifest(yaml).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
    }

    #[rstest::rstest]
    #[case("MyApp")]
    #[case("my_app")]
    #[case("my.app")]
    #[case("my app")]
    #[case("MY-APP")]
    #[case("app@prod")]
    fn test_parse_manifest_rejects_invalid_names(#[case] name: &str) {
        let yaml = format!(
            "apiVersion: v1\nmetadata:\n  name: {}\nenvironment:\n  workingDir: /workspaces/my-app\n",
            name
        );
        let err = parse_manifest(&yaml).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
    }

    #[rstest::rstest]
    #[case("my-app")]
    #[case("myapp")]
    #[case("my-app-123")]
    #[case("a")]
    #[case("0")]
    #[case("---")]
    #[case("a-b-c")]
    fn test_parse_manifest_accepts_valid_names(#[case] name: &str) {
        let yaml = format!(
            "apiVersion: v1\nmetadata:\n  name: {}\nenvironment:\n  workingDir: /workspaces/my-app\n",
            name
        );
        parse_manifest(&yaml).expect("valid name should parse");
    }

    #[test]
    fn test_parse_manifest_rejects_relative_working_dir() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: my-app
environment:
  workingDir: relative/path
"#;
        let err = parse_manifest(yaml).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
        let msg = err.to_string().to_lowercase();
        assert!(msg.contains("workingdir") || msg.contains("absolute"));
    }

    #[test]
    fn test_parse_manifest_rejects_duplicate_health_checks() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: my-app
environment:
  workingDir: /workspaces/my-app
  healthChecks:
    - name: dup
      command: echo 1
    - name: dup
      command: echo 2
"#;
        let err = parse_manifest(yaml).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
        assert!(err.to_string().contains("duplicate health check"));
    }

    #[test]
    fn test_parse_manifest_rejects_duplicate_secrets() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: my-app
environment:
  workingDir: /workspaces/my-app
  secrets:
    - name: DUP
    - name: DUP
"#;
        let err = parse_manifest(yaml).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
        assert!(err.to_string().contains("duplicate secret"));
    }

    #[test]
    fn test_parse_manifest_rejects_requires_health_unknown_ref() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: my-app
environment:
  workingDir: /workspaces/my-app
  healthChecks:
    - name: db_ready
      command: pg_isready
commands:
  migrate:
    command: ./migrate.sh
    requiresHealth:
      - db_ready
      - nonexistent_check
"#;
        let err = parse_manifest(yaml).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
        assert!(err.to_string().contains("requiresHealth"));
        assert!(err.to_string().contains("nonexistent_check"));
    }

    #[test]
    fn test_parse_manifest_accepts_no_commands() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: my-app
environment:
  workingDir: /workspaces/my-app
"#;
        let m = parse_manifest(yaml).expect("manifest without commands should parse");
        assert!(m.commands.is_empty());
    }

    #[test]
    fn test_parse_manifest_accepts_no_hooks() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: my-app
environment:
  workingDir: /workspaces/my-app
"#;
        let m = parse_manifest(yaml).expect("manifest without hooks should parse");
        assert!(m.hooks.is_none());
    }

    #[test]
    fn test_parse_manifest_accepts_hooks_present() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: my-app
environment:
  workingDir: /workspaces/my-app
hooks:
  postStart:
    - command: echo hi
  preStop:
    - command: echo bye
"#;
        let m = parse_manifest(yaml).expect("manifest with hooks should parse");
        let hooks = m.hooks.expect("hooks should be present");
        assert_eq!(hooks.post_start.len(), 1);
        assert_eq!(hooks.pre_stop.len(), 1);
    }

    #[test]
    fn test_parse_manifest_accepts_no_secrets() {
        let yaml = r#"
apiVersion: v1
metadata:
  name: my-app
environment:
  workingDir: /workspaces/my-app
"#;
        let m = parse_manifest(yaml).expect("manifest without secrets should parse");
        assert!(m.environment.secrets.is_empty());
    }

    #[test]
    fn test_parse_manifest_from_file_reads_disk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("CODESPACE.yaml");
        std::fs::write(&path, MINIMAL_MANIFEST).expect("write");
        let m = parse_manifest_from_file(&path).expect("should parse");
        assert_eq!(m.metadata.name, "my-app");
    }

    #[test]
    fn test_parse_manifest_from_file_errors_on_missing_file() {
        let path = std::path::Path::new("/tmp/codespacectl-nonexistent-9f3b7c/CODESPACE.yaml");
        let err = parse_manifest_from_file(path).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
        assert!(err.to_string().contains("failed to read"));
    }

    #[test]
    fn test_parse_manifest_from_file_errors_on_invalid_content() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("CODESPACE.yaml");
        std::fs::write(
            &path,
            "apiVersion: v2\nmetadata:\n  name: x\nenvironment:\n  workingDir: /x\n",
        )
        .unwrap();
        let err = parse_manifest_from_file(&path).unwrap_err();
        assert_eq!(err.kind(), "manifest_version_unsupported");
    }

    #[test]
    fn test_validate_manifest_accepts_minimal_valid() {
        let m = parse_manifest(MINIMAL_MANIFEST).expect("minimal should parse");
        validate_manifest(&m).expect("minimal manifest should validate");
    }

    #[test]
    fn test_validate_manifest_rejects_unsupported_version() {
        let m = Manifest {
            api_version: "v2".into(),
            metadata: Metadata {
                name: "my-app".into(),
                description: None,
                repo: None,
            },
            environment: Environment {
                working_dir: "/workspaces/my-app".into(),
                health_checks: vec![],
                secrets: vec![],
            },
            commands: HashMap::new(),
            hooks: None,
        };
        let err = validate_manifest(&m).unwrap_err();
        assert_eq!(err.kind(), "manifest_version_unsupported");
    }

    #[test]
    fn test_validate_manifest_rejects_empty_name() {
        let m = Manifest {
            api_version: "v1".into(),
            metadata: Metadata {
                name: "".into(),
                description: None,
                repo: None,
            },
            environment: Environment {
                working_dir: "/workspaces/my-app".into(),
                health_checks: vec![],
                secrets: vec![],
            },
            commands: HashMap::new(),
            hooks: None,
        };
        let err = validate_manifest(&m).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
        assert!(err.to_string().to_lowercase().contains("name"));
    }

    #[test]
    fn test_validate_manifest_rejects_uppercase_name() {
        let m = Manifest {
            api_version: "v1".into(),
            metadata: Metadata {
                name: "Upper".into(),
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
        let err = validate_manifest(&m).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
    }

    #[test]
    fn test_validate_manifest_rejects_relative_working_dir() {
        let m = Manifest {
            api_version: "v1".into(),
            metadata: Metadata {
                name: "my-app".into(),
                description: None,
                repo: None,
            },
            environment: Environment {
                working_dir: "relative/path".into(),
                health_checks: vec![],
                secrets: vec![],
            },
            commands: HashMap::new(),
            hooks: None,
        };
        let err = validate_manifest(&m).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
    }

    #[test]
    fn test_validate_manifest_rejects_duplicate_health_check() {
        let m = Manifest {
            api_version: "v1".into(),
            metadata: Metadata {
                name: "my-app".into(),
                description: None,
                repo: None,
            },
            environment: Environment {
                working_dir: "/x".into(),
                health_checks: vec![
                    HealthCheck {
                        name: "dup".into(),
                        command: "echo 1".into(),
                        expect_exit_code: 0,
                        timeout_secs: 30,
                    },
                    HealthCheck {
                        name: "dup".into(),
                        command: "echo 2".into(),
                        expect_exit_code: 0,
                        timeout_secs: 30,
                    },
                ],
                secrets: vec![],
            },
            commands: HashMap::new(),
            hooks: None,
        };
        let err = validate_manifest(&m).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
    }

    #[test]
    fn test_validate_manifest_rejects_duplicate_secret() {
        let m = Manifest {
            api_version: "v1".into(),
            metadata: Metadata {
                name: "my-app".into(),
                description: None,
                repo: None,
            },
            environment: Environment {
                working_dir: "/x".into(),
                health_checks: vec![],
                secrets: vec![
                    Secret {
                        name: "DUP".into(),
                        required: false,
                        generate_if_missing: None,
                    },
                    Secret {
                        name: "DUP".into(),
                        required: false,
                        generate_if_missing: None,
                    },
                ],
            },
            commands: HashMap::new(),
            hooks: None,
        };
        let err = validate_manifest(&m).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
    }

    #[test]
    fn test_validate_manifest_rejects_unknown_requires_health() {
        let mut commands = HashMap::new();
        commands.insert(
            "migrate".into(),
            Command {
                description: None,
                command: "./m.sh".into(),
                timeout_secs: 300,
                requires_health: vec!["does-not-exist".into()],
                idempotent: false,
            },
        );
        let m = Manifest {
            api_version: "v1".into(),
            metadata: Metadata {
                name: "my-app".into(),
                description: None,
                repo: None,
            },
            environment: Environment {
                working_dir: "/x".into(),
                health_checks: vec![],
                secrets: vec![],
            },
            commands,
            hooks: None,
        };
        let err = validate_manifest(&m).unwrap_err();
        assert_eq!(err.kind(), "manifest_invalid");
        assert!(err.to_string().contains("does-not-exist"));
    }

    #[test]
    fn test_validate_manifest_accepts_command_referencing_existing_health() {
        let mut commands = HashMap::new();
        commands.insert(
            "migrate".into(),
            Command {
                description: None,
                command: "./m.sh".into(),
                timeout_secs: 300,
                requires_health: vec!["db_ready".into()],
                idempotent: false,
            },
        );
        let m = Manifest {
            api_version: "v1".into(),
            metadata: Metadata {
                name: "my-app".into(),
                description: None,
                repo: None,
            },
            environment: Environment {
                working_dir: "/x".into(),
                health_checks: vec![HealthCheck {
                    name: "db_ready".into(),
                    command: "pg_isready".into(),
                    expect_exit_code: 0,
                    timeout_secs: 30,
                }],
                secrets: vec![],
            },
            commands,
            hooks: None,
        };
        validate_manifest(&m).expect("valid manifest with command referencing health check");
    }
}
