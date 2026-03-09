use garde::Validate;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

fn default_port() -> u16 {
    3000
}
fn default_max_concurrent() -> u16 {
    10
}
fn default_runtime() -> String {
    "tmux".to_string()
}
fn default_agent() -> String {
    "claude-code".to_string()
}
fn default_workspace() -> String {
    "worktree".to_string()
}
fn default_tracker() -> String {
    "github".to_string()
}
fn default_branch() -> String {
    "main".to_string()
}
fn default_max_turns() -> u16 {
    20
}
fn default_sandbox() -> String {
    "workspace-write".to_string()
}

#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "default_port")]
    #[garde(skip)]
    pub port: u16,

    #[serde(default)]
    #[garde(skip)]
    pub defaults: Defaults,

    #[garde(custom(validate_projects))]
    pub projects: HashMap<String, ProjectConfig>,

    #[serde(default = "default_max_concurrent")]
    #[garde(skip)]
    pub max_concurrent_agents: u16,

    // Post-MVP optional fields — accepted but not validated
    #[serde(default)]
    #[garde(skip)]
    pub terminal_port: Option<u16>,
    #[serde(default)]
    #[garde(skip)]
    pub ready_threshold_ms: Option<u64>,
    #[serde(default)]
    #[garde(skip)]
    pub max_session_tokens: Option<u64>,
    #[serde(default)]
    #[garde(skip)]
    pub max_session_wall_clock_ms: Option<u64>,
    #[serde(default)]
    #[garde(skip)]
    pub max_retries_per_issue: Option<u16>,
    #[serde(default)]
    #[garde(skip)]
    pub notifiers: Option<HashMap<String, Value>>,
    #[serde(default)]
    #[garde(skip)]
    pub reactions: Option<Value>,
}

impl Config {
    /// Inherit top-level defaults into per-project fields.
    pub fn resolve_defaults(&mut self) {
        for project in self.projects.values_mut() {
            if project.runtime.is_none() {
                project.runtime = Some(self.defaults.runtime.clone());
            }
            if project.agent.is_none() {
                project.agent = Some(self.defaults.agent.clone());
            }
            if project.workspace.is_none() {
                project.workspace = Some(self.defaults.workspace.clone());
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default = "default_agent")]
    pub agent: String,
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default)]
    pub notifiers: Vec<String>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            runtime: default_runtime(),
            agent: default_agent(),
            workspace: default_workspace(),
            notifiers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub name: Option<String>,
    pub repo: String,
    pub path: String,

    #[serde(default = "default_branch")]
    pub default_branch: String,

    #[serde(default)]
    pub session_prefix: Option<String>,

    pub runtime: Option<String>,
    pub agent: Option<String>,
    pub workspace: Option<String>,

    #[serde(default)]
    pub tracker: TrackerConfig,

    #[serde(default)]
    pub symlinks: Vec<String>,

    #[serde(default)]
    pub hooks: Hooks,

    #[serde(default)]
    pub agent_config: Option<AgentConfig>,

    pub agent_rules: Option<String>,
    pub agent_rules_file: Option<String>,
    pub orchestrator_rules: Option<String>,

    #[serde(default)]
    pub reactions: Option<Value>,
}

fn is_valid_repo_format(repo: &str) -> bool {
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 {
        return false;
    }
    let valid_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-';
    parts[0].chars().all(valid_char)
        && !parts[0].is_empty()
        && parts[1].chars().all(valid_char)
        && !parts[1].is_empty()
}

fn validate_projects(projects: &HashMap<String, ProjectConfig>, _ctx: &()) -> garde::Result {
    if projects.is_empty() {
        return Err(garde::Error::new("projects must not be empty"));
    }
    for (name, project) in projects {
        if !is_valid_repo_format(&project.repo) {
            return Err(garde::Error::new(format!(
                "project '{name}': repo '{}' must match owner/repo format",
                project.repo
            )));
        }
        let expanded = shellexpand::tilde(&project.path);
        let p = std::path::Path::new(expanded.as_ref());
        if !p.exists() {
            return Err(garde::Error::new(format!(
                "project '{name}': path does not exist: {}",
                project.path
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackerConfig {
    #[serde(default = "default_tracker")]
    pub plugin: String,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub active_states: Vec<String>,
    #[serde(default)]
    pub terminal_states: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Hooks {
    pub after_create: Option<String>,
    pub before_run: Option<String>,
    pub after_run: Option<String>,
    pub before_remove: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    #[serde(default = "default_max_turns")]
    pub max_turns: u16,
    #[serde(default)]
    pub permissions: Option<String>,
    pub model: Option<String>,
    #[serde(default = "default_sandbox")]
    pub sandbox: String,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Secrets from environment variables. Never serialized.
#[derive(Debug)]
pub struct ResolvedSecrets {
    pub linear_api_key: Option<String>,
    pub slack_webhook_url: Option<String>,
    pub composio_api_key: Option<String>,
}
