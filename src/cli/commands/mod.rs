//! CLI subcommand handlers.
//!
//! Each handler takes `&Cli` (so it has access to global `--json`, `--manifest`,
//! `--verbose` flags) and the per-subcommand args it needs. `main.rs` does the
//! dispatch by pattern-matching on `Commands` and forwarding.

pub mod common;
pub mod connect;
pub mod discover;
pub mod doctor;
pub mod exec;
pub mod health;
pub mod init;
pub mod raw;
pub mod session;
pub mod state;
pub mod stop;
pub mod switch;
pub mod token;
