//! CLI argument parsing and command dispatch.

pub mod args;
pub mod commands;
pub mod output;

pub use args::{Cli, Commands};
pub use output::{print_envelope, OutputEnvelope, SessionRef};
// Re-export ErrorEnvelope from error module for convenience
pub use crate::error::ErrorEnvelope;
