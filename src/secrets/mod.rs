//! Secrets: encrypted-at-rest storage using `age`.
//!
//! Wave 4 subagent (parallel): implement.

pub mod generation;
pub mod storage;

pub use generation::generate_secret;
pub use storage::{SecretError, SecretStore};
