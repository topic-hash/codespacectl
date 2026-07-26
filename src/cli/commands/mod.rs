//! CLI subcommand handlers.
//!
//! Each handler takes `&Cli` (so it has access to global `--json`, `--manifest`,
//! `--verbose` flags) and the per-subcommand args it needs. `main.rs` does the
//! dispatch by pattern-matching on `Commands` and forwarding.

pub mod common;
pub mod init;
pub mod discover;
pub mod switch;
pub mod connect;
pub mod health;
pub mod exec;
pub mod raw;
pub mod stop;
pub mod state;
pub mod session;
pub mod doctor;
pub mod token;
