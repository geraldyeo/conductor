mod commands;
mod error;
mod ipc;
mod output;
mod resolve;

use clap::{Parser, Subcommand};
use error::{exit_code, print_error, CliError};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ao", about = "Agent Orchestrator CLI", version)]
struct Cli {
    #[arg(long, global = true, help = "JSON output")]
    json: bool,
    #[arg(long, short = 'p', global = true, help = "Project ID override")]
    project: Option<String>,
    #[arg(long, global = true, help = "Config file path override")]
    config: Option<PathBuf>,
    #[arg(long, short = 'v', global = true, help = "Verbose logging")]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate agent-orchestrator.yaml
    Init {
        #[arg(long, help = "Auto mode: infer from git remote + CWD")]
        auto: bool,
        #[arg(long, short = 'o', help = "Output path")]
        output: Option<PathBuf>,
    },
    /// Start orchestrator (poll loop + IPC listener)
    Start {
        #[arg(long, help = "Accepted but no-op at MVP")]
        no_dashboard: bool,
    },
    /// Stop orchestrator
    Stop,
    /// Show session table
    Status,
    /// Spawn agent session for an issue
    Spawn {
        issue_url: String,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, help = "Open terminal after spawn")]
        open: bool,
    },
    /// Spawn sessions for multiple issues
    BatchSpawn {
        issue_urls: Vec<String>,
        #[arg(long)]
        agent: Option<String>,
    },
    /// Send a message to a session
    Send {
        session_id: String,
        message: Vec<String>,
        #[arg(long, short = 'f')]
        file: Option<PathBuf>,
        #[arg(long)]
        no_wait: bool,
        #[arg(long, default_value = "600")]
        timeout: u64,
    },
    /// Session subcommands
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
}

#[derive(Subcommand)]
enum SessionCmd {
    /// List sessions (alias for status)
    Ls,
    /// Kill a session
    Kill { session_id: String },
    /// Kill sessions with terminal tracker state
    Cleanup {
        #[arg(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    let result = run(cli).await;
    if let Err(e) = result {
        let json = std::env::args().any(|a| a == "--json");
        print_error(&e, json);
        std::process::exit(exit_code(&e));
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Commands::Init { auto, output } => commands::init::run(auto, output, cli.json).await,
        Commands::Start { .. } => {
            commands::start::run(cli.project.as_deref(), cli.config.as_deref(), cli.json).await
        }
        Commands::Stop => commands::stop::run(cli.json).await,
        Commands::Status => {
            commands::status::run(cli.project.as_deref(), cli.config.as_deref(), cli.json).await
        }
        Commands::Spawn {
            issue_url,
            agent,
            open,
        } => {
            commands::spawn::run(
                &issue_url,
                agent.as_deref(),
                open,
                cli.project.as_deref(),
                cli.config.as_deref(),
                cli.json,
            )
            .await
        }
        Commands::BatchSpawn { issue_urls, agent } => {
            commands::batch_spawn::run(
                issue_urls,
                agent.as_deref(),
                cli.project.as_deref(),
                cli.config.as_deref(),
                cli.json,
            )
            .await
        }
        Commands::Send {
            session_id,
            message,
            file,
            no_wait,
            timeout,
        } => {
            let content = build_send_content(message, file).await?;
            commands::send::run(&session_id, &content, no_wait, timeout, cli.json).await
        }
        Commands::Session { cmd } => match cmd {
            SessionCmd::Ls => {
                commands::status::run(cli.project.as_deref(), cli.config.as_deref(), cli.json).await
            }
            SessionCmd::Kill { session_id } => commands::session::kill(&session_id, cli.json).await,
            SessionCmd::Cleanup { dry_run } => {
                commands::session::cleanup(
                    dry_run,
                    cli.project.as_deref(),
                    cli.config.as_deref(),
                    cli.json,
                )
                .await
            }
        },
    }
}

async fn build_send_content(args: Vec<String>, file: Option<PathBuf>) -> Result<String, CliError> {
    if let Some(path) = file {
        return tokio::fs::read_to_string(&path).await.map_err(CliError::Io);
    }
    if !args.is_empty() {
        return Ok(args.join(" "));
    }
    // Read from stdin if not a TTY
    use tokio::io::AsyncReadExt;
    let mut buf = String::new();
    tokio::io::stdin()
        .read_to_string(&mut buf)
        .await
        .map_err(CliError::Io)?;
    Ok(buf.trim().to_string())
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
