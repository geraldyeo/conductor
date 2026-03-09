use crate::error::CliError;
use crate::ipc::client::send_request;
use crate::resolve::resolve_project;
use conductor_core::ipc::{OrchestratorRequest, OrchestratorResponse};
use std::path::Path;

pub async fn kill(session_id: &str, json: bool) -> Result<(), CliError> {
    let req = OrchestratorRequest::Kill {
        session_id: session_id.to_string(),
    };
    let _response = send_request(&req).await?;

    if json {
        println!(
            "{}",
            serde_json::json!({"session_id": session_id, "status": "kill_scheduled"})
        );
    } else {
        println!("Kill scheduled for {session_id}.");
    }
    Ok(())
}

pub async fn cleanup(
    dry_run: bool,
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

    let req = OrchestratorRequest::Cleanup {
        project_id: project_id.to_string(),
        dry_run,
    };

    let response = send_request(&req).await?;

    if let OrchestratorResponse::CleanupResult { killed, skipped } = response {
        if json {
            println!(
                "{}",
                serde_json::json!({"killed": killed, "skipped": skipped})
            );
        } else {
            println!("Killed: {}", killed.join(", "));
            println!("Skipped: {}", skipped.join(", "));
        }
    }

    Ok(())
}
