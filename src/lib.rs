//! # codespacectl
//!
//! Manifest-driven CLI for agent-driven GitHub Codespace operations.
//!
//! Single Rust binary. No system SSH required. No daemon. Token from env var.
//! Codespace identity passed by name; cached in state file.
//!
//! See `docs/ARCHITECTURE.md` for design overview.

pub mod cli;
pub mod error;
pub mod exec;
pub mod github;
pub mod health;
pub mod manifest;
pub mod session;
pub mod secrets;
pub mod ssh;
pub mod state;

pub use error::{CodespaceError, Result};
pub use github::{GithubApiClient, ShellExecutor};
