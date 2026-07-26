//! codespacectl — manifest-driven CLI for agent-driven GitHub Codespace operations.

use clap::Parser;
use codespacectl::cli::{Cli, Commands};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize tracing based on verbosity.
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
        Commands::Init { path } => {
            codespacectl::cli::commands::init::handle(cli, path).await
        }
        Commands::Discover => codespacectl::cli::commands::discover::handle(cli).await,
        Commands::Connect { .. } => codespacectl::cli::commands::connect::handle(cli).await,
        Commands::Health { .. } => codespacectl::cli::commands::health::handle(cli).await,
        Commands::Exec { .. } => codespacectl::cli::commands::exec::handle(cli).await,
        Commands::Raw { .. } => codespacectl::cli::commands::raw::handle(cli).await,
        Commands::Stop { .. } => codespacectl::cli::commands::stop::handle(cli).await,
        Commands::State { .. } => codespacectl::cli::commands::state::handle(cli).await,
        Commands::Session(_) => codespacectl::cli::commands::session::handle(cli).await,
        Commands::Doctor => codespacectl::cli::commands::doctor::handle(cli).await,
        Commands::Token(_) => codespacectl::cli::commands::token::handle(cli).await,
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
                eprintln!("  -> {}", e.suggested_action());
            }
            exit_code
        }
    }
}
