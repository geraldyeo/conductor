use anyhow::{Context, Result};
use conductor_core::agent::ClaudeCodeAgent;
use conductor_core::orchestrator::{Orchestrator, OrchestratorConfig};
use conductor_core::runtime::TmuxRuntime;
use conductor_core::utils::{CommandRunner, DataPaths};
use std::path::Path;
use std::sync::Arc;

pub async fn run_spawn(
    issue_url: &str,
    branch: Option<&str>,
    prompt: Option<&str>,
    repo: Option<&Path>,
    json_output: bool,
) -> Result<()> {
    // Resolve repo root
    let repo_root = match repo {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir().context("cannot determine current directory")?,
    };

    // Derive branch name from issue URL if not provided
    let branch_name = match branch {
        Some(b) => b.to_string(),
        None => derive_branch_name(issue_url),
    };

    // Default prompt if not provided
    let prompt_text = prompt.unwrap_or("Complete the GitHub issue described below.");

    // Set up DataPaths
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let data_root = home.join(".agent-orchestrator");
    let paths = DataPaths::from_root(data_root);

    // Build orchestrator
    let agent = Arc::new(ClaudeCodeAgent::new());
    let runtime = Arc::new(TmuxRuntime::new(Arc::new(CommandRunner)));
    let config = OrchestratorConfig {
        data_paths: paths,
        repo_root: repo_root.clone(),
    };
    let orchestrator = Orchestrator::new(config, agent, runtime);

    // Execute spawn
    tracing::info!(%issue_url, %branch_name, "spawning agent session");
    let session_id = orchestrator
        .spawn(issue_url, &branch_name, prompt_text)
        .await
        .with_context(|| format!("failed to spawn session for {issue_url}"))?;

    // Output result
    if json_output {
        println!(
            "{}",
            serde_json::json!({
                "session_id": session_id,
                "issue_url": issue_url,
                "branch": branch_name,
                "status": "spawned",
            })
        );
    } else {
        println!("Session spawned: {session_id}");
        println!("  Issue: {issue_url}");
        println!("  Branch: {branch_name}");
        println!("  Attach: tmux attach -t {session_id}");
    }

    Ok(())
}

/// Derive a branch name from an issue URL.
/// e.g. "https://github.com/org/repo/issues/123" -> "ao/issue-123"
fn derive_branch_name(issue_url: &str) -> String {
    let number = issue_url
        .split('/')
        .next_back()
        .and_then(|s| s.split('#').next()) // strip URL fragments like #issuecomment-123
        .and_then(|s| s.parse::<u64>().ok())
        .map(|n| n.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!("ao/issue-{number}")
}

#[cfg(test)]
mod tests {
    use super::*;

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
