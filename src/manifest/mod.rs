//! CODESPACE.yaml manifest parser, schema, and template renderer.
//!
//! Public types and functions are pre-declared here. Implementation is filled in
//! by Wave 2 subagent. See `docs/MANIFEST_SPEC.md` for the full schema spec.

use std::collections::HashMap;
use std::path::Path;

pub mod parser;
pub mod schema;
pub mod templates;

pub use parser::{parse_manifest, parse_manifest_from_file, validate_manifest};
pub use schema::*;
pub use templates::{render_template, TemplateContext};

/// Loaded manifest with its source path and content hash.
#[derive(Debug, Clone)]
pub struct LoadedManifest {
    pub manifest: Manifest,
    pub source_path: std::path::PathBuf,
    pub sha256: String,
    pub loaded_at: chrono::DateTime<chrono::Utc>,
}

/// Compute the SHA-256 hash of the manifest content (for change detection).
pub fn manifest_sha256(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Find a CODESPACE.yaml file by walking up from the current directory.
/// Returns the first match, or an error if none found before reaching root.
pub fn find_manifest(start_dir: &Path) -> crate::Result<std::path::PathBuf> {
    let mut current = start_dir.to_path_buf();
    loop {
        let candidate = current.join("CODESPACE.yaml");
        if candidate.exists() && candidate.is_file() {
            return Ok(candidate);
        }
        let candidate_yml = current.join("CODESPACE.yml");
        if candidate_yml.exists() && candidate_yml.is_file() {
            return Ok(candidate_yml);
        }
        if !current.pop() {
            return Err(crate::CodespaceError::ManifestNotFound(format!(
                "no CODESPACE.yaml found walking up from {}",
                start_dir.display()
            )));
        }
    }
}

// Re-export commonly used types
pub use LoadedManifest as Loaded;

/// Empty placeholder — keeps module compiling while subagent fills in.
#[allow(dead_code)]
fn _force_compile_link() {
    let _ = Manifest {
        api_version: String::new(),
        metadata: Metadata {
            name: String::new(),
            description: None,
            repo: None,
        },
        environment: Environment {
            working_dir: String::new(),
            health_checks: vec![],
            secrets: vec![],
        },
        commands: HashMap::new(),
        hooks: None,
    };
}
