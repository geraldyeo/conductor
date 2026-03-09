use anyhow::{Context, Result};
use conductor_core::ipc::{OrchestratorRequest, OrchestratorResponse};
use std::path::Path;

use crate::ipc::client::send_request;

pub async fn run_spawn(
    issue_url: &str,
    _branch: Option<&str>,
    _prompt: Option<&str>,
    _repo: Option<&Path>,
    json_output: bool,
) -> Result<()> {
    // TODO: resolve project_id from repo path or config
    let project_id = "default".to_string();

    let request = OrchestratorRequest::Spawn {
        project_id,
        issue_url: issue_url.to_string(),
        agent: None,
        open: false,
    };

    let response = send_request(&request)
        .await
        .context("failed to communicate with orchestrator")?;

    match response {
        OrchestratorResponse::SpawnResult {
            session_id,
            branch,
            workspace_path,
        } => {
            if json_output {
                println!(
                    "{}",
                    serde_json::json!({
                        "session_id": session_id,
                        "issue_url": issue_url,
                        "branch": branch,
                        "workspace_path": workspace_path,
                        "status": "spawned",
                    })
                );
            } else {
                println!("Session spawned: {session_id}");
                println!("  Issue: {issue_url}");
                println!("  Branch: {branch}");
                println!("  Workspace: {workspace_path}");
                println!("  Attach: tmux attach -t {session_id}");
            }
        }
        other => {
            anyhow::bail!("unexpected response: {other:?}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use conductor_core::utils::parse_github_issue_number;

    /// Derive a branch name from an issue URL.
    fn derive_branch_name(issue_url: &str) -> String {
        let number = parse_github_issue_number(issue_url)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        format!("ao/issue-{number}")
    }

    #[test]
    fn test_derive_branch_name() {
        assert_eq!(
            derive_branch_name("https://github.com/org/repo/issues/42"),
            "ao/issue-42"
        );
    }

    #[test]
    fn test_derive_branch_name_invalid_url() {
        assert_eq!(derive_branch_name("not-a-url"), "ao/issue-unknown");
    }
}
