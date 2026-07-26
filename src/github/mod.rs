//! GitHub Codespaces API client.
//!
//! Uses `reqwest` with rustls. Token from `$CODESPACECTL_TOKEN`.
//! All methods return `CodespaceError` variants on failure.

pub mod client;
pub mod codespaces;
pub mod auth;
pub mod gh_downloader;
pub mod traits;

pub use client::GitHubClient;
pub use codespaces::{CodespaceInfo, CodespaceState as ApiCodespaceState};
pub use gh_downloader::{ensure_gh_binary, find_gh_binary, cached_gh_path};
pub use traits::{GithubApiClient, ShellExecutor};
