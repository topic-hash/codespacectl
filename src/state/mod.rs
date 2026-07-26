//! Persisted state at `~/.cache/codespacectl/state.json`.
//!
//! Tracks: current codespace, current manifest, manifest SHA, per-codespace
//! SSH host keys, last-known states, last health check status, token fingerprint.

pub mod file;
pub mod codespace;
pub mod manifest_state;

pub use file::{load_state, save_state, state_dir, state_file_path, export_state, import_state};
pub use codespace::CodespaceState;
pub use manifest_state::ManifestState;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level state structure, persisted as JSON.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    /// State file schema version (currently 1).
    pub version: u32,

    /// Name of the last `connect`ed codespace. Used by `exec` without --codespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_codespace: Option<String>,

    /// Path of the last loaded manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_manifest: Option<String>,

    /// SHA-256 of the last loaded manifest (for change detection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_manifest_sha256: Option<String>,

    /// Per-codespace state, keyed by full codespace name.
    #[serde(default)]
    pub codespaces: HashMap<String, CodespaceState>,

    /// Per-manifest state, keyed by manifest path.
    #[serde(default)]
    pub manifests: HashMap<String, ManifestState>,

    /// sha256(token)[:8] — for revocation detection (never persist full token).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_fingerprint: Option<String>,
}
