//! Manifest parser and validator. Filled in by Wave 2 subagent.
//!
//! TODO (subagent): implement `parse_manifest`, `parse_manifest_from_file`,
//! `validate_manifest` per the spec in `docs/MANIFEST_SPEC.md`.

use super::schema::Manifest;
use std::path::Path;
use crate::Result;

/// Parse a manifest from a YAML string.
pub fn parse_manifest(content: &str) -> Result<Manifest> {
    let manifest: Manifest = serde_yaml::from_str(content)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

/// Load and parse a manifest from a file path.
pub fn parse_manifest_from_file(path: &Path) -> Result<Manifest> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| crate::CodespaceError::ManifestInvalid(format!(
            "failed to read {}: {}",
            path.display(),
            e
        )))?;
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
    if !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
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
