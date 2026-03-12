use crate::error::CliError;
use std::path::PathBuf;

pub async fn run(auto: bool, output: Option<PathBuf>, json: bool) -> Result<(), CliError> {
    let (project_id, repo, path) = if auto {
        let id = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "myproject".to_string());
        let repo = get_git_remote().unwrap_or_else(|| "owner/repo".to_string());
        let path = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        (id, repo, path)
    } else {
        prompt_interactively()?
    };

    let yaml = conductor_core::config::generate_default(&project_id, &repo, &path);
    let out_path = output.unwrap_or_else(|| PathBuf::from("agent-orchestrator.yaml"));

    if out_path.exists() {
        return Err(CliError::General(format!(
            "config already exists at {out_path:?}. Use -o to specify a different path."
        )));
    }

    std::fs::write(&out_path, &yaml).map_err(CliError::Io)?;

    if json {
        println!(
            "{}",
            serde_json::json!({"created": out_path.to_string_lossy()})
        );
    } else {
        println!("Created {out_path:?}");
        println!("Edit the file to fill in your project details, then run: ao start");
    }

    Ok(())
}

fn get_git_remote() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    let url = String::from_utf8(output.stdout).ok()?;
    parse_github_repo_from_url(url.trim())
}

fn parse_github_repo_from_url(url: &str) -> Option<String> {
    // Handle SSH: git@github.com:owner/repo.git -> owner/repo
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return Some(rest.trim_end_matches(".git").to_string());
    }
    // Handle HTTPS: https://github.com/owner/repo.git -> owner/repo
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        return Some(rest.trim_end_matches(".git").to_string());
    }
    None
}

fn prompt_interactively() -> Result<(String, String, String), CliError> {
    // Fall back to auto-detection; interactive prompts are post-MVP.
    let id = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "myproject".to_string());
    let repo = get_git_remote().unwrap_or_else(|| "owner/repo".to_string());
    let path = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    Ok((id, repo, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_repo_from_ssh_url() {
        assert_eq!(
            parse_github_repo_from_url("git@github.com:owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_repo_from_https_url() {
        assert_eq!(
            parse_github_repo_from_url("https://github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_repo_from_https_url_without_git_suffix() {
        assert_eq!(
            parse_github_repo_from_url("https://github.com/owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_repo_returns_none_for_unknown_format() {
        assert_eq!(parse_github_repo_from_url("https://gitlab.com/owner/repo"), None);
    }

    #[test]
    fn test_prompt_interactively_returns_ok() {
        // Without --auto, init should fall back to auto-detection rather than error.
        let result = prompt_interactively();
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        let (id, repo, path) = result.unwrap();
        assert!(!id.is_empty());
        assert!(!repo.is_empty());
        assert!(!path.is_empty());
    }
}
