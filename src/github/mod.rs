//! GitHub Codespaces API client.
//!
//! Uses `reqwest` with rustls. Token from `$CODESPACECTL_TOKEN`.
//! All methods return `CodespaceError` variants on failure.

pub mod client;
pub mod codespaces;
pub mod auth;

pub use client::GitHubClient;
pub use codespaces::{CodespaceInfo, CodespaceState as ApiCodespaceState};
