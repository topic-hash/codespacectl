//! CLI argument definitions (clap).

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "codespacectl",
    version,
    about = "Manifest-driven CLI for agent-driven GitHub Codespace operations",
    long_about = "codespacectl is a single-binary Rust CLI for connecting AI agents to GitHub Codespaces via a declarative CODESPACE.yaml manifest. No system SSH, no daemon. Token from $CODESPACECTL_TOKEN.",
    propagate_version = true
)]
pub struct Cli {
    /// Output JSON envelope instead of human-readable text
    #[arg(long, global = true)]
    pub json: bool,

    /// Increase verbosity (use -v, -vv, -vvv)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Path to CODESPACE.yaml manifest (auto-discovered if omitted)
    #[arg(long, global = true)]
    pub manifest: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Register a manifest by path or URL
    Init {
        /// Path or URL to the CODESPACE.yaml file
        path: String,
    },

    /// List codespaces for the authenticated user. With `--json` returns an array
    /// suitable for selection. Combine with `codespacectl switch` to change current.
    Discover {
        /// Filter by repository (e.g. "topic-hash/DataMigrata"). Optional.
        #[arg(long)]
        repo: Option<String>,

        /// Filter by state (e.g. "Available"). Optional.
        #[arg(long)]
        state: Option<String>,
    },

    /// Switch the current codespace. Without args: lists all codespaces for selection
    /// (interactive in TTY, JSON array otherwise). With `--codespace <name>`: sets that
    /// codespace as current in state (does NOT connect — use `connect` after).
    Switch {
        /// Codespace name to switch to (skips the selection list)
        #[arg(long)]
        codespace: Option<String>,

        /// Pick the Nth entry from the discovery list (1-indexed). Useful for agents.
        #[arg(long)]
        index: Option<usize>,
    },

    /// Connect to a codespace (start, hooks, health check)
    Connect {
        /// Codespace name (full or partial — partial matches the first one)
        #[arg(long)]
        codespace: String,

        /// Accept a new host key on first connect or after rotation (default true on first connect; for rotation, requires explicit flag)
        #[arg(long)]
        accept_new_host_key: bool,

        /// Skip health checks (use with caution)
        #[arg(long)]
        skip_health: bool,

        /// Skip postStart hooks
        #[arg(long)]
        skip_hooks: bool,

        /// Timeout in seconds for codespace start (default 180)
        #[arg(long, default_value_t = 180)]
        timeout: u64,
    },

    /// Run manifest health checks
    Health {
        /// Codespace name (uses current if omitted)
        #[arg(long)]
        codespace: Option<String>,
    },

    /// Execute a manifest-declared command
    Exec {
        /// Command name from the manifest
        command: String,

        /// Codespace name (uses current if omitted)
        #[arg(long)]
        codespace: Option<String>,

        /// Skip the health gate (run even if health is red)
        #[arg(long)]
        force: bool,

        /// Override the command's timeout
        #[arg(long)]
        timeout: Option<u64>,
    },

    /// Execute an ad-hoc shell command
    Raw {
        /// Shell command to execute
        command: String,

        /// Codespace name (uses current if omitted)
        #[arg(long)]
        codespace: Option<String>,

        /// Timeout in seconds
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },

    /// Stop the codespace (run preStop hooks first)
    Stop {
        /// Codespace name (uses current if omitted)
        #[arg(long)]
        codespace: Option<String>,

        /// Skip preStop hooks
        #[arg(long)]
        skip_hooks: bool,
    },

    /// Show persisted state
    State {
        /// Export state as JSON for cross-machine transfer
        #[arg(long)]
        export: bool,

        /// Import state from JSON (replaces existing state)
        #[arg(long)]
        import: Option<String>,
    },

    /// Session log operations
    #[command(subcommand)]
    Session(SessionCommands),

    /// Diagnose environment issues
    Doctor,

    /// Token management
    #[command(subcommand)]
    Token(TokenCommands),
}

#[derive(Subcommand, Debug)]
pub enum SessionCommands {
    /// Show recent sessions
    Log {
        /// Number of recent sessions to show
        #[arg(long, default_value_t = 5)]
        last: usize,

        /// Show full contents of a specific session by ID
        #[arg(long)]
        session: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum TokenCommands {
    /// Save a token (read from stdin, no echo) to the token file
    Set,

    /// Show the token file path (does not display the token itself)
    Get,

    /// Remove the token file
    Clear,
}
