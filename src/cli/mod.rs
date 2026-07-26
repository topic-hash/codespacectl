//! CLI argument parsing and command dispatch.

pub mod args;
pub mod output;
pub mod commands;

pub use args::{Cli, Commands};
pub use output::{OutputEnvelope, SessionRef, print_envelope};
// Re-export ErrorEnvelope from error module for convenience
pub use crate::error::ErrorEnvelope;
