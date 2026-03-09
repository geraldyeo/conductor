use crate::error::CliError;
use crate::ipc::client::send_request;
use crate::resolve::resolve_project;
use conductor_core::ipc::{OrchestratorRequest, OrchestratorResponse};
use std::path::Path;

pub async fn run(
    issue_url: &str,
    agent: Option<&str>,
    open: bool,
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
        resolve_project(&config, project).map_err(|e| CliError::General(e.to_string()))?;

    let req = OrchestratorRequest::Spawn {
        project_id: project_id.to_string(),
        issue_url: issue_url.to_string(),
        agent: agent.map(String::from),
        open,
    };

    let response = send_request(&req).await?;

    match response {
        OrchestratorResponse::SpawnResult {
            session_id,
            branch,
            workspace_path,
        } => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "session_id": session_id,
                        "branch": branch,
                        "workspace_path": workspace_path,
                    })
                );
            } else {
                println!("Session spawned: {session_id}");
                println!("  Branch: {branch}");
                println!("  Workspace: {workspace_path}");
                println!("  Attach: tmux attach -t {session_id}");
            }
        }
        other => {
            return Err(CliError::General(format!("unexpected response: {other:?}")));
        }
    }

    Ok(())
}
