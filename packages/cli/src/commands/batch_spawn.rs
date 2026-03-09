use crate::error::CliError;
use crate::ipc::client::send_request;
use crate::resolve::resolve_project;
use conductor_core::ipc::{BatchSpawnOutcome, OrchestratorRequest, OrchestratorResponse};
use std::path::Path;

pub async fn run(
    issue_urls: Vec<String>,
    agent: Option<&str>,
    project: Option<&str>,
    config_path: Option<&Path>,
    json: bool,
) -> Result<(), CliError> {
    let config = if let Some(p) = config_path {
        conductor_core::config::load_from_path(p)?
    } else {
        conductor_core::config::load()?
    };

    let project_id =
        resolve_project(&config, project)?;

    let req = OrchestratorRequest::BatchSpawn {
        project_id: project_id.to_string(),
        issue_urls,
        agent: agent.map(String::from),
        open: false,
    };

    let response = send_request(&req).await?;

    if let OrchestratorResponse::BatchSpawnResult { results } = response {
        if json {
            println!(
                "{}",
                serde_json::to_string(&results)
                    .map_err(|e| CliError::General(format!("JSON serialization failed: {e}")))?
            );
        } else {
            for item in &results {
                match &item.outcome {
                    BatchSpawnOutcome::Spawned { session_id, branch } => {
                        println!("spawned  {} -> {} ({})", item.issue_url, session_id, branch);
                    }
                    BatchSpawnOutcome::Skipped { reason } => {
                        println!("skipped  {} -- {}", item.issue_url, reason);
                    }
                    BatchSpawnOutcome::Failed { error } => {
                        eprintln!("failed   {} -- {}", item.issue_url, error);
                    }
                }
            }
        }
    }

    Ok(())
}
