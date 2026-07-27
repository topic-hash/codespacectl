//! Subcommand handler — `codespacectl init <path|URL>`.
//!
//! Registers a manifest by copying (or fetching) its content into
//! `~/.cache/codespacectl/manifests/<sha256>.yaml`, validating it, and
//! recording its SHA-256 in the state file.

use crate::cli::{print_envelope, Cli, OutputEnvelope};
use crate::manifest::{manifest_sha256, parse_manifest};
use crate::state::{load_state, save_state, ManifestState};
use crate::CodespaceError;
use serde::Serialize;
use std::path::PathBuf;

/// JSON-serializable result for the `init` command (used by `--json` output).
#[derive(Debug, Serialize)]
struct InitResult {
    name: String,
    sha256: String,
    cached_path: String,
    manifest_count: usize,
}

/// Handle the `init` subcommand.
///
/// `path` is either a local file path or an `http(s)://` URL. The content is
/// fetched/copied, hashed, validated, and cached under
/// `~/.cache/codespacectl/manifests/<sha256>.yaml`. The state file is updated
/// with `state.manifests[name] = ManifestState { sha256, last_validated_at }`
/// and the `current_manifest` / `current_manifest_sha256` pointers are set.
pub async fn handle(args: &Cli, path: &str) -> crate::Result<i32> {
    // 1. Fetch content from URL or read local file.
    let (content, source_label) = if path.starts_with("http://") || path.starts_with("https://") {
        let resp = reqwest::get(path).await?;
        if !resp.status().is_success() {
            return Err(CodespaceError::NetworkError(format!(
                "failed to fetch {}: HTTP {}",
                path,
                resp.status()
            )));
        }
        let text = resp.text().await?;
        (text, path.to_string())
    } else {
        let p = PathBuf::from(path);
        if !p.exists() {
            return Err(CodespaceError::ManifestNotFound(format!(
                "manifest file not found: {}",
                path
            )));
        }
        let text = std::fs::read_to_string(&p)?;
        (text, p.display().to_string())
    };

    // 2. Compute SHA-256 of the content.
    let sha = manifest_sha256(&content);

    // 3. Parse + validate manifest (so we surface schema errors at register time).
    let manifest = parse_manifest(&content)?;
    let name = manifest.metadata.name.clone();

    // 4. Cache the content at ~/.cache/codespacectl/manifests/<sha>.yaml.
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp/.cache"))
        .join("codespacectl")
        .join("manifests");
    std::fs::create_dir_all(&cache_dir)?;
    let cached_path = cache_dir.join(format!("{}.yaml", sha));
    std::fs::write(&cached_path, &content)?;

    // 5. Update state.
    let mut state = load_state()?;
    state.manifests.insert(
        name.clone(),
        ManifestState {
            sha256: Some(sha.clone()),
            last_validated_at: Some(chrono::Utc::now().to_rfc3339()),
        },
    );
    state.current_manifest = Some(cached_path.display().to_string());
    state.current_manifest_sha256 = Some(sha.clone());
    let manifest_count = state.manifests.len();
    save_state(&state)?;

    // 6. Emit output.
    let result_data = InitResult {
        name: name.clone(),
        sha256: sha.clone(),
        cached_path: cached_path.display().to_string(),
        manifest_count,
    };

    if args.json {
        let envelope = OutputEnvelope::success(result_data);
        print_envelope(envelope);
    } else {
        println!("Registered manifest '{}' from {}", name, source_label);
        println!("  SHA-256:        {}", result_data.sha256);
        println!("  Cached at:      {}", result_data.cached_path);
        println!("  Total registered: {}", result_data.manifest_count);
    }

    Ok(0)
}
