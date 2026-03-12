mod discovery;
pub mod error;
pub mod schema;
mod secrets;

pub use error::ConfigError;
pub use schema::{
    AgentConfig, Config, Defaults, Hooks, ProjectConfig, ResolvedSecrets, TrackerConfig,
};
pub use secrets::load_secrets;

use garde::Validate;
use std::path::{Path, PathBuf};

/// Discover config path, load, validate. Primary entry point.
pub fn load() -> Result<Config, ConfigError> {
    let path = discovery::discover_config_path()?;
    load_from_path(&path)
}

/// Like `load()` but also returns the discovered config file path.
/// Use this when the path is needed downstream (e.g. for `DataPaths` hashing).
pub fn load_with_path() -> Result<(Config, PathBuf), ConfigError> {
    let path = discovery::discover_config_path()?;
    let config = load_from_path(&path)?;
    Ok((config, path))
}

/// Load from a specific path (for tests and `ao init --check`).
pub fn load_from_path(path: &Path) -> Result<Config, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;

    let mut config: Config = serde_yml::from_str(&content).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        source: e,
    })?;

    config.resolve_defaults();

    config.validate(&()).map_err(|e| ConfigError::Validation {
        path: path.to_path_buf(),
        violations: e.to_string(),
    })?;

    Ok(config)
}

/// Generate a default config YAML string for `ao init`.
pub fn generate_default(project_id: &str, repo: &str, path: &str) -> String {
    format!(
        r#"port: 3000
maxConcurrentAgents: 10

defaults:
  runtime: tmux
  agent: claude-code
  workspace: worktree

projects:
  {project_id}:
    repo: {repo}
    path: {path}
    defaultBranch: main
    tracker:
      plugin: github
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_config(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_load_valid_config() {
        // path validation requires the path to exist
        let path = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let yaml = format!(
            r#"projects:
  myapp:
    repo: owner/myapp
    path: "{path}"
"#
        );
        let f = write_config(&yaml);
        let config = load_from_path(f.path()).unwrap();
        assert!(config.projects.contains_key("myapp"));
        // defaults should be resolved
        assert_eq!(
            config.projects["myapp"].agent.as_deref(),
            Some("claude-code")
        );
    }

    #[test]
    fn test_load_empty_projects_fails() {
        let yaml = "projects: {}\n";
        let f = write_config(yaml);
        let result = load_from_path(f.path());
        assert!(matches!(result, Err(ConfigError::Validation { .. })));
    }

    #[test]
    fn test_load_bad_repo_format_fails() {
        let path = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let yaml = format!(
            r#"projects:
  myapp:
    repo: just-a-name
    path: "{path}"
"#
        );
        let f = write_config(&yaml);
        let result = load_from_path(f.path());
        assert!(matches!(result, Err(ConfigError::Validation { .. })));
    }

    #[test]
    fn test_generate_default_roundtrips() {
        // generate_default produces valid YAML that can be re-parsed structurally
        let yaml = generate_default("myapp", "owner/myapp", "/tmp");
        let config: serde_yml::Value = serde_yml::from_str(&yaml).unwrap();
        assert!(config["projects"]["myapp"]["repo"].as_str().is_some());
    }

    #[test]
    fn test_tracker_default_terminal_states_includes_closed() {
        let path = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        // Config with no terminalStates field — default must include "closed"
        let yaml = format!(
            r#"projects:
  myapp:
    repo: owner/myapp
    path: "{path}"
    tracker:
      plugin: github
"#
        );
        let f = write_config(&yaml);
        let config = load_from_path(f.path()).unwrap();
        let terminal_states = &config.projects["myapp"].tracker.terminal_states;
        assert!(
            terminal_states
                .iter()
                .any(|s| s.eq_ignore_ascii_case("closed")),
            "terminal_states should include 'closed' by default, got: {:?}",
            terminal_states
        );
    }

    #[test]
    fn test_resolve_defaults_fills_missing_project_fields() {
        let path = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let yaml = format!(
            r#"defaults:
  runtime: process
  agent: claude-code
  workspace: worktree
projects:
  myapp:
    repo: owner/myapp
    path: "{path}"
"#
        );
        let f = write_config(&yaml);
        let config = load_from_path(f.path()).unwrap();
        assert_eq!(config.projects["myapp"].runtime.as_deref(), Some("process"));
    }
}
