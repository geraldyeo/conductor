mod spawn;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ao", about = "Agent Orchestrator CLI", version)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Spawn an agent session for a GitHub issue
    Spawn {
        /// GitHub issue URL
        issue_url: String,
        /// Git branch name (auto-derived from issue URL if omitted)
        #[arg(long)]
        branch: Option<String>,
        /// Custom prompt for the agent
        #[arg(long)]
        prompt: Option<String>,
        /// Path to the repository root
        #[arg(long)]
        repo: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Spawn {
            issue_url,
            branch,
            prompt,
            repo,
        } => {
            if let Err(e) = spawn::run_spawn(
                &issue_url,
                branch.as_deref(),
                prompt.as_deref(),
                repo.as_deref(),
                cli.json,
            )
            .await
            {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_parses() {
        Cli::command().debug_assert();
    }
}
