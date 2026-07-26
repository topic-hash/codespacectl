//! codespacectl — manifest-driven CLI for agent-driven GitHub Codespace operations.

use clap::Parser;
use codespacectl::cli::{Cli, Commands};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize tracing based on verbosity
    let filter = match cli.verbose {
        0 => "codespacectl=warn",
        1 => "codespacectl=info",
        2 => "codespacectl=debug",
        _ => "codespacectl=trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let exit_code = dispatch(&cli).await;

    std::process::exit(exit_code);
}

async fn dispatch(cli: &Cli) -> i32 {
    let result = match &cli.command {
        Commands::Init { .. } => codespacectl::cli::commands::init::handle().await,
        Commands::Discover => codespacectl::cli::commands::discover::handle().await,
        Commands::Connect { .. } => codespacectl::cli::commands::connect::handle().await,
        Commands::Health { .. } => codespacectl::cli::commands::health::handle().await,
        Commands::Exec { .. } => codespacectl::cli::commands::exec::handle().await,
        Commands::Raw { .. } => codespacectl::cli::commands::raw::handle().await,
        Commands::Stop { .. } => codespacectl::cli::commands::stop::handle().await,
        Commands::State { .. } => codespacectl::cli::commands::state::handle().await,
        Commands::Session(_) => codespacectl::cli::commands::session::handle().await,
        Commands::Doctor => codespacectl::cli::commands::doctor::handle().await,
        Commands::Token(_) => codespacectl::cli::commands::token::handle().await,
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            let exit_code = e.exit_code();
            if cli.json {
                let envelope = codespacectl::cli::OutputEnvelope::<()>::failure(e);
                codespacectl::cli::print_envelope(envelope);
            } else {
                eprintln!("error: {}", e);
                eprintln!("  → {}", e.suggested_action());
            }
            exit_code
        }
    }
}
