//! Secrets: encrypted-at-rest storage using `age`.
//!
//! Wave 4 subagent (parallel): implement.

pub mod storage;
pub mod generation;

pub use storage::{SecretStore, SecretError};
pub use generation::generate_secret;
