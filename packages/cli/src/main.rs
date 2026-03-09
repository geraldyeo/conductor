use clap::{Parser, Subcommand};
use conductor_core::ipc::{send_request, OrchestratorRequest, OrchestratorResponse};

#[derive(Parser)]
#[command(name = "ao", about = "Agent Orchestrator")]
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
        /// GitHub issue URL or number
        issue: String,
        /// Project ID (auto-detected if only one project)
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Start the orchestrator
    Start,
    /// Stop the orchestrator
    Stop,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Spawn { issue, project } => {
            let issue_id = parse_issue_id(&issue);
            let project_id = project.unwrap_or_else(|| "default".to_string());
            let req = OrchestratorRequest::Spawn {
                project_id,
                issue_id,
            };
            match send_request(&req).await {
                Ok(OrchestratorResponse::SpawnResult { session_id, branch }) => {
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::json!({"session_id": session_id, "branch": branch})
                        );
                    } else {
                        println!("Session started: {session_id}");
                        println!("Branch: {branch}");
                    }
                }
                Ok(OrchestratorResponse::Error { code, message }) => {
                    eprintln!("error [{code}]: {message}");
                    std::process::exit(1);
                }
                Ok(other) => {
                    eprintln!("unexpected response: {other:?}");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(4);
                }
            }
        }
        Commands::Start => {
            eprintln!("ao start: not yet implemented in walking skeleton");
            eprintln!("Run spawn_session() directly for now.");
            std::process::exit(1);
        }
        Commands::Stop => {
            let req = OrchestratorRequest::Stop;
            match send_request(&req).await {
                Ok(_) => println!("Orchestrator stopped."),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(4);
                }
            }
        }
    }
}

/// Parse an issue identifier from various formats:
/// - "42" → "42"
/// - "#42" → "42"
/// - "https://github.com/owner/repo/issues/42" → "42"
fn parse_issue_id(issue: &str) -> String {
    if let Some(id) = issue.strip_prefix('#') {
        return id.to_string();
    }
    if issue.contains('/') {
        if let Some(id) = issue.rsplit('/').next() {
            return id.to_string();
        }
    }
    issue.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_issue_id_plain_number() {
        assert_eq!(parse_issue_id("42"), "42");
    }

    #[test]
    fn test_parse_issue_id_hash_prefix() {
        assert_eq!(parse_issue_id("#42"), "42");
    }

    #[test]
    fn test_parse_issue_id_full_url() {
        assert_eq!(
            parse_issue_id("https://github.com/owner/repo/issues/42"),
            "42"
        );
    }

    #[test]
    fn test_parse_issue_id_short_url() {
        assert_eq!(parse_issue_id("owner/repo/issues/99"), "99");
    }
}
