# Full MVP Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the remaining 9 CLI commands, poll cycle, 16-state lifecycle engine, IPC server, and config system to complete the Conductor MVP.

**Architecture:** 3 phases. Phase 1 tasks are parallelizable (non-overlapping file ownership). Phase 2 refactors the orchestrator to wire it all together. Phase 3 completes the CLI.

**Tech Stack:** Rust, tokio, clap v4 (derive), serde/serde_yml/serde_json, garde, shellexpand, async-trait, thiserror, strum, dirs

---

## Phase 1A — Parallel tasks (no file conflicts)

Tasks 1, 2, 3 can run concurrently in separate worktrees branched from `main`.

---

## Task 1: Config System

**Owns:**
- Create: `packages/core/src/config/mod.rs`
- Create: `packages/core/src/config/schema.rs`
- Create: `packages/core/src/config/discovery.rs`
- Create: `packages/core/src/config/validation.rs`
- Create: `packages/core/src/config/secrets.rs`
- Create: `packages/core/src/config/error.rs`
- Modify: `packages/core/src/lib.rs` (add `pub mod config;`)
- Modify: `packages/core/Cargo.toml` (add `serde_yml`, `garde`, `shellexpand`)
- Modify: `Cargo.toml` (workspace: add `garde = "0.18"`, `shellexpand = "3"`)

**Step 1: Add workspace dependencies**

In `Cargo.toml` (workspace root), add to `[workspace.dependencies]`:
```toml
garde = { version = "0.18", features = ["derive", "full"] }
shellexpand = "3"
serde_yml = "0.0.12"
```

`serde_yml` is already there; add only `garde` and `shellexpand`.

**Step 2: Add to core's Cargo.toml**

In `packages/core/Cargo.toml`, add to `[dependencies]`:
```toml
serde_yml = { workspace = true }
garde = { workspace = true }
shellexpand = { workspace = true }
```

**Step 3: Create `packages/core/src/config/error.rs`**

```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config not found (searched CWD ancestors and home directory)")]
    NotFound,
    #[error("AO_CONFIG_PATH={0:?} does not exist")]
    EnvPathNotFound(PathBuf),
    #[error("could not determine home directory")]
    NoHomeDir,
    #[error("{path}: parse error: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yml::Error,
    },
    #[error("{path}: validation failed:\n{violations}")]
    Validation {
        path: PathBuf,
        violations: String,
    },
    #[error("{path}: {message}")]
    Io {
        path: PathBuf,
        message: String,
    },
}
```

**Step 4: Create `packages/core/src/config/schema.rs`**

```rust
use garde::Validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

fn default_port() -> u16 { 3000 }
fn default_max_concurrent() -> u16 { 10 }
fn default_runtime() -> String { "tmux".to_string() }
fn default_agent() -> String { "claude-code".to_string() }
fn default_workspace() -> String { "worktree".to_string() }
fn default_tracker() -> String { "github".to_string() }
fn default_branch() -> String { "main".to_string() }
fn default_max_turns() -> u16 { 20 }
fn default_sandbox() -> String { "workspace-write".to_string() }

#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default = "default_port")]
    #[garde(skip)]
    pub port: u16,

    #[serde(default)]
    #[garde(skip)]
    pub defaults: Defaults,

    #[garde(length(min = 1))]
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

#[derive(Debug, Clone, Deserialize, Default)]
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

#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    #[garde(skip)]
    pub name: Option<String>,

    #[garde(pattern(r"^[a-zA-Z0-9_.\-]+/[a-zA-Z0-9_.\-]+$"))]
    pub repo: String,

    #[garde(custom(validate_path_exists))]
    pub path: String,

    #[serde(default = "default_branch")]
    #[garde(skip)]
    pub default_branch: String,

    #[serde(default)]
    #[garde(skip)]
    pub session_prefix: Option<String>,

    #[garde(skip)]
    pub runtime: Option<String>,
    #[garde(skip)]
    pub agent: Option<String>,
    #[garde(skip)]
    pub workspace: Option<String>,

    #[serde(default)]
    #[garde(skip)]
    pub tracker: TrackerConfig,

    #[serde(default)]
    #[garde(skip)]
    pub symlinks: Vec<String>,

    #[serde(default)]
    #[garde(skip)]
    pub hooks: Hooks,

    #[serde(default)]
    #[garde(skip)]
    pub agent_config: Option<AgentConfig>,

    #[garde(skip)]
    pub agent_rules: Option<String>,
    #[garde(skip)]
    pub agent_rules_file: Option<String>,
    #[garde(skip)]
    pub orchestrator_rules: Option<String>,

    #[serde(default)]
    #[garde(skip)]
    pub reactions: Option<Value>,
}

fn validate_path_exists(path: &str, _ctx: &()) -> garde::Result {
    let expanded = shellexpand::tilde(path);
    let p = std::path::Path::new(expanded.as_ref());
    if p.exists() {
        Ok(())
    } else {
        Err(garde::Error::new(format!("path does not exist: {path}")))
    }
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
```

**Step 5: Create `packages/core/src/config/discovery.rs`**

```rust
use super::error::ConfigError;
use std::path::PathBuf;

pub fn discover_config_path() -> Result<PathBuf, ConfigError> {
    // 1. Env var override
    if let Ok(val) = std::env::var("AO_CONFIG_PATH") {
        let p = PathBuf::from(val);
        if p.exists() {
            return Ok(p);
        }
        return Err(ConfigError::EnvPathNotFound(p));
    }

    // 2. Walk up from CWD
    let cwd = std::env::current_dir().map_err(|e| ConfigError::Io {
        path: PathBuf::from("."),
        message: e.to_string(),
    })?;
    let mut dir = cwd.as_path();
    loop {
        for name in ["agent-orchestrator.yaml", "agent-orchestrator.yml"] {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }

    // 3. Home directory fallback
    let home = dirs::home_dir().ok_or(ConfigError::NoHomeDir)?;
    for path in [
        home.join(".agent-orchestrator.yaml"),
        home.join(".agent-orchestrator.yml"),
        home.join(".config/agent-orchestrator/config.yaml"),
    ] {
        if path.exists() {
            return Ok(path);
        }
    }

    Err(ConfigError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_env_override_missing_file() {
        env::set_var("AO_CONFIG_PATH", "/nonexistent/path.yaml");
        let result = discover_config_path();
        env::remove_var("AO_CONFIG_PATH");
        assert!(matches!(result, Err(ConfigError::EnvPathNotFound(_))));
    }
}
```

**Step 6: Create `packages/core/src/config/secrets.rs`**

```rust
use super::schema::ResolvedSecrets;

pub fn load_secrets() -> ResolvedSecrets {
    ResolvedSecrets {
        linear_api_key: std::env::var("LINEAR_API_KEY").ok(),
        slack_webhook_url: std::env::var("SLACK_WEBHOOK_URL").ok(),
        composio_api_key: std::env::var("COMPOSIO_API_KEY").ok(),
    }
}
```

**Step 7: Create `packages/core/src/config/mod.rs`**

```rust
pub mod error;
pub mod schema;
mod discovery;
mod secrets;

pub use error::ConfigError;
pub use schema::{
    AgentConfig, Config, Defaults, Hooks, ProjectConfig, ResolvedSecrets, TrackerConfig,
};
pub use secrets::load_secrets;

use garde::Validate;
use std::path::Path;

/// Discover config path, load, validate. Primary entry point.
pub fn load() -> Result<Config, ConfigError> {
    let path = discovery::discover_config_path()?;
    load_from_path(&path)
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
        let path = std::env::current_dir().unwrap().to_string_lossy().to_string();
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
        assert_eq!(config.projects["myapp"].agent.as_deref(), Some("claude-code"));
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
        let path = std::env::current_dir().unwrap().to_string_lossy().to_string();
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
    fn test_resolve_defaults_fills_missing_project_fields() {
        let path = std::env::current_dir().unwrap().to_string_lossy().to_string();
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
```

**Step 8: Modify `packages/core/src/lib.rs`**

Add `pub mod config;` to the existing module list. Final file:
```rust
pub mod config;
pub mod types;

pub mod agent;
pub mod ipc;
pub mod orchestrator;
pub mod plugins;
pub mod prompt;
pub mod runtime;
pub mod session_store;
pub mod utils;
```

**Step 9: Run tests**

```bash
/Users/geraldyeo/.cargo/bin/cargo test -p conductor-core --lib config
```
Expected: all tests in the config module pass.

**Step 10: Run clippy**

```bash
/Users/geraldyeo/.cargo/bin/cargo clippy -p conductor-core -- -D warnings
```
Expected: no warnings.

**Step 11: Commit**

```bash
git add packages/core/src/config/ packages/core/src/lib.rs packages/core/Cargo.toml Cargo.toml
git commit -m "feat(config): implement YAML config system (ADR-0003)"
```

---

## Task 2: SessionStore `list()`

**Owns:**
- Modify: `packages/core/src/session_store/mod.rs` (add `list()` method)

No Cargo.toml changes, no lib.rs changes. Pure addition to an existing file.

**Step 1: Write the failing test**

In `packages/core/src/session_store/mod.rs`, add inside the `#[cfg(test)]` block:

```rust
#[tokio::test]
async fn test_list_returns_all_sessions() {
    let dir = tempdir().unwrap();
    let store = make_store(dir.path());

    let m1 = make_metadata("sess-1");
    let m2 = make_metadata("sess-2");
    store.create_session(&m1).await.unwrap();
    store.create_session(&m2).await.unwrap();

    let sessions = store.list().await.unwrap();
    assert_eq!(sessions.len(), 2);
    let ids: Vec<_> = sessions.iter().map(|s| s.id.clone()).collect();
    assert!(ids.contains(&"sess-1".to_string()));
    assert!(ids.contains(&"sess-2".to_string()));
}

#[tokio::test]
async fn test_list_skips_malformed_metadata() {
    let dir = tempdir().unwrap();
    let store = make_store(dir.path());

    let m = make_metadata("good-sess");
    store.create_session(&m).await.unwrap();

    // Create a session dir with no metadata file (simulates partial write)
    let bad_dir = store.paths.session_dir("bad-sess");
    std::fs::create_dir(&bad_dir).unwrap();

    let sessions = store.list().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "good-sess");
}

#[tokio::test]
async fn test_list_empty_store() {
    let dir = tempdir().unwrap();
    let store = make_store(dir.path());
    let sessions = store.list().await.unwrap();
    assert!(sessions.is_empty());
}

#[tokio::test]
async fn test_list_filters_by_non_terminal_status() {
    let dir = tempdir().unwrap();
    let store = make_store(dir.path());

    let mut m1 = make_metadata("active-sess");
    m1.status = SessionStatus::Working;
    let mut m2 = make_metadata("dead-sess");
    m2.status = SessionStatus::Killed;

    store.create_session(&m1).await.unwrap();
    store.create_session(&m2).await.unwrap();

    // list_active filters terminal sessions
    let active = store.list_active().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "active-sess");
}
```

**Step 2: Run test to verify it fails**

```bash
/Users/geraldyeo/.cargo/bin/cargo test -p conductor-core session_store::tests::test_list_returns_all_sessions 2>&1 | tail -5
```
Expected: FAIL with "no method named `list` found"

**Step 3: Implement `list()` and `list_active()` in `SessionStore`**

Add these methods to the `impl SessionStore` block (after the existing `read_journal` method):

```rust
/// List all sessions by reading every metadata file under sessions_dir.
/// Sessions with unreadable/malformed metadata are silently skipped.
pub async fn list(&self) -> Result<Vec<SessionMetadata>, StoreError> {
    let sessions_dir = self.paths.sessions_dir();
    let mut sessions = Vec::new();

    let mut read_dir = match fs::read_dir(&sessions_dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(StoreError::Io(e)),
    };

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let session_id = match path.file_name().and_then(|n| n.to_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };
        match self.read_metadata(&session_id).await {
            Ok(meta) => sessions.push(meta),
            Err(e) => {
                tracing::warn!(session_id = %session_id, error = %e, "skipping unreadable session");
            }
        }
    }

    Ok(sessions)
}

/// List only non-terminal sessions.
pub async fn list_active(&self) -> Result<Vec<SessionMetadata>, StoreError> {
    let all = self.list().await?;
    Ok(all.into_iter().filter(|s| !s.status.is_terminal()).collect())
}
```

Also add `is_terminal()` to `SessionStatus` in `packages/core/src/types/status.rs`:

```rust
impl SessionStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SessionStatus::Killed
                | SessionStatus::Terminated
                | SessionStatus::Done
                | SessionStatus::Cleanup
                | SessionStatus::Errored
                | SessionStatus::Merged
        )
    }
}
```

Add a test for `is_terminal` in `status.rs`:
```rust
#[test]
fn test_is_terminal() {
    assert!(SessionStatus::Killed.is_terminal());
    assert!(SessionStatus::Merged.is_terminal());
    assert!(!SessionStatus::Working.is_terminal());
    assert!(!SessionStatus::Spawning.is_terminal());
}
```

**Step 4: Run tests**

```bash
/Users/geraldyeo/.cargo/bin/cargo test -p conductor-core 2>&1 | tail -15
```
Expected: all tests pass.

**Step 5: Commit**

```bash
git add packages/core/src/session_store/mod.rs packages/core/src/types/status.rs
git commit -m "feat(session-store): add list(), list_active(), SessionStatus::is_terminal()"
```

---

## Task 3: IPC Protocol Types + Client

**Owns:**
- Modify: `packages/core/src/ipc/mod.rs` (add request/response types + framing helpers)
- Create: `packages/cli/src/ipc/mod.rs`
- Create: `packages/cli/src/ipc/client.rs`

No conflicts with Tasks 1 or 2.

**Step 1: Write tests for IPC serde round-trip**

In `packages/core/src/ipc/mod.rs`, add at the end:
```rust
#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[test]
    fn test_spawn_request_roundtrip() {
        let req = OrchestratorRequest::Spawn {
            project_id: "myapp".to_string(),
            issue_url: "https://github.com/org/repo/issues/42".to_string(),
            agent: None,
            open: false,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: OrchestratorRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, OrchestratorRequest::Spawn { .. }));
    }

    #[test]
    fn test_spawn_result_response_roundtrip() {
        let resp = OrchestratorResponse::SpawnResult {
            session_id: "ao-abc123".to_string(),
            branch: "feat/issue-42".to_string(),
            workspace_path: "/tmp/worktrees/ao-abc123".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: OrchestratorResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, OrchestratorResponse::SpawnResult { .. }));
    }

    #[test]
    fn test_error_response_roundtrip() {
        let resp = OrchestratorResponse::Error {
            code: "issue_terminal".to_string(),
            message: "issue is closed".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: OrchestratorResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, OrchestratorResponse::Error { .. }));
    }
}
```

**Step 2: Run test to verify it fails**

```bash
/Users/geraldyeo/.cargo/bin/cargo test -p conductor-core ipc::protocol_tests 2>&1 | tail -5
```
Expected: FAIL with "cannot find type `OrchestratorRequest`"

**Step 3: Implement IPC types and framing in `packages/core/src/ipc/mod.rs`**

Replace the current `mod.rs` entirely:

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Returns the path to the orchestrator Unix domain socket.
pub fn socket_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".agent-orchestrator").join("orchestrator.sock")
}

// ── Request / Response types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorRequest {
    Spawn {
        project_id: String,
        issue_url: String,
        agent: Option<String>,
        open: bool,
    },
    BatchSpawn {
        project_id: String,
        issue_urls: Vec<String>,
        agent: Option<String>,
        open: bool,
    },
    Send {
        session_id: String,
        content: String,
        no_wait: bool,
        timeout_secs: u64,
    },
    Kill {
        session_id: String,
    },
    Cleanup {
        project_id: String,
        dry_run: bool,
    },
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorResponse {
    Ok {
        message: String,
    },
    SpawnResult {
        session_id: String,
        branch: String,
        workspace_path: String,
    },
    BatchSpawnResult {
        results: Vec<BatchSpawnItem>,
    },
    CleanupResult {
        killed: Vec<String>,
        skipped: Vec<String>,
    },
    SendResult {
        delivered: bool,
        activity_state: String,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSpawnItem {
    pub issue_url: String,
    pub outcome: BatchSpawnOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum BatchSpawnOutcome {
    Spawned { session_id: String, branch: String },
    Skipped { reason: String },
    Failed { error: String },
}

// ── Length-prefixed JSON framing ─────────────────────────────────────────────
// Wire format: 4-byte big-endian u32 length prefix, then UTF-8 JSON body.

pub async fn write_message<T: Serialize, W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    value: &T,
) -> std::io::Result<()> {
    let json = serde_json::to_vec(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let len = json.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&json).await?;
    writer.flush().await
}

pub async fn read_message<T: for<'de> Deserialize<'de>, R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> std::io::Result<T> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 4 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("IPC message too large: {len} bytes"),
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_path_is_absolute() {
        let path = socket_path();
        assert!(path.is_absolute());
    }

    #[test]
    fn test_socket_path_filename() {
        let path = socket_path();
        assert_eq!(path.file_name().and_then(|f| f.to_str()), Some("orchestrator.sock"));
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    // ... (tests from Step 1 above)
}
```

**Step 4: Create `packages/cli/src/ipc/mod.rs`**

```rust
mod client;
pub use client::{send_request, IpcError};
```

**Step 5: Create `packages/cli/src/ipc/client.rs`**

```rust
use conductor_core::ipc::{read_message, socket_path, write_message, OrchestratorRequest, OrchestratorResponse};
use thiserror::Error;
use tokio::net::UnixStream;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("orchestrator is not running (socket not found at {0:?})")]
    NotRunning(std::path::PathBuf),
    #[error("IPC error: {0}")]
    Io(#[from] std::io::Error),
    #[error("orchestrator returned error: [{code}] {message}")]
    Orchestrator { code: String, message: String },
}

/// Send a request to the running orchestrator and return the response.
/// Returns `IpcError::NotRunning` if the socket doesn't exist or connection is refused.
pub async fn send_request(request: &OrchestratorRequest) -> Result<OrchestratorResponse, IpcError> {
    let path = socket_path();

    if !path.exists() {
        return Err(IpcError::NotRunning(path));
    }

    let mut stream = UnixStream::connect(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused {
            IpcError::NotRunning(path.clone())
        } else {
            IpcError::Io(e)
        }
    })?;

    write_message(&mut stream, request).await?;
    let response: OrchestratorResponse = read_message(&mut stream).await?;

    if let OrchestratorResponse::Error { code, message } = response {
        return Err(IpcError::Orchestrator { code, message });
    }

    Ok(response)
}
```

**Step 6: Add `thiserror` to cli's Cargo.toml**

In `packages/cli/Cargo.toml`:
```toml
thiserror = { workspace = true }
```

**Step 7: Run tests**

```bash
/Users/geraldyeo/.cargo/bin/cargo test -p conductor-core ipc 2>&1 | tail -10
/Users/geraldyeo/.cargo/bin/cargo build -p cli 2>&1 | tail -10
```
Expected: core IPC tests pass, cli compiles.

**Step 8: Commit**

```bash
git add packages/core/src/ipc/mod.rs packages/cli/src/ipc/ packages/cli/Cargo.toml
git commit -m "feat(ipc): add OrchestratorRequest/Response types and length-framed JSON protocol"
```

---

## Phase 1B — After Task 1 merges

---

## Task 4: Lifecycle Engine

**Prerequisite:** Task 1 (Config) merged to `main`. Branch from updated `main`.

**Owns:**
- Create: `packages/core/src/lifecycle/mod.rs`
- Create: `packages/core/src/lifecycle/graph.rs`
- Create: `packages/core/src/lifecycle/poll.rs`
- Modify: `packages/core/src/lib.rs` (add `pub mod lifecycle;`)

**Step 1: Write failing tests**

Create `packages/core/src/lifecycle/graph.rs` with tests first:

```rust
// Write the test module:
#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx() -> PollContext {
        PollContext {
            runtime_alive: true,
            activity_state: ActivityState::Active,
            pr: None,
            tracker_state: TrackerState::Active,
            budget_exceeded: false,
            manual_kill: false,
        }
    }

    #[test]
    fn test_spawning_to_working_when_alive_and_active() {
        let graph = StateGraph::build();
        let ctx = make_ctx();
        let next = graph.evaluate(SessionStatus::Spawning, &ctx);
        assert_eq!(next, Some(SessionStatus::Working));
    }

    #[test]
    fn test_spawning_to_errored_when_dead() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.runtime_alive = false;
        ctx.activity_state = ActivityState::Idle;
        let next = graph.evaluate(SessionStatus::Spawning, &ctx);
        assert_eq!(next, Some(SessionStatus::Errored));
    }

    #[test]
    fn test_working_to_stuck_when_idle() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.activity_state = ActivityState::Idle;
        let next = graph.evaluate(SessionStatus::Working, &ctx);
        assert_eq!(next, Some(SessionStatus::Stuck));
    }

    #[test]
    fn test_global_kill_edge_fires_from_working() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.manual_kill = true;
        let next = graph.evaluate(SessionStatus::Working, &ctx);
        assert_eq!(next, Some(SessionStatus::Killed));
    }

    #[test]
    fn test_global_cleanup_edge_fires_from_stuck() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.activity_state = ActivityState::Idle; // would normally go to Stuck
        ctx.tracker_state = TrackerState::Terminal;
        // cleanup (precedence 29) fires before stuck (precedence 5)? No — cleanup is a global edge.
        // Global edges are appended; local edges have lower precedence numbers (win first).
        // stuck is precedence 5, global cleanup is precedence 29. stuck wins.
        // But cleanup fires from stuck: it's appended to stuck's node too.
        // So from Working + idle + terminal: Working → Stuck (prec 5) fires first.
        // To test cleanup from Stuck directly:
        let mut ctx2 = make_ctx();
        ctx2.activity_state = ActivityState::Idle;
        ctx2.tracker_state = TrackerState::Terminal;
        // evaluate from Stuck state
        let next = graph.evaluate(SessionStatus::Stuck, &ctx2);
        assert_eq!(next, Some(SessionStatus::Cleanup));
    }

    #[test]
    fn test_no_transition_for_terminal_node() {
        let graph = StateGraph::build();
        let ctx = make_ctx();
        let next = graph.evaluate(SessionStatus::Killed, &ctx);
        assert_eq!(next, None);
    }

    #[test]
    fn test_graph_validation_passes() {
        // build() panics on invalid graph — so this passing means it's valid
        let _graph = StateGraph::build();
    }
}
```

**Step 2: Run test to verify it fails**

```bash
/Users/geraldyeo/.cargo/bin/cargo test -p conductor-core lifecycle 2>&1 | tail -5
```
Expected: FAIL with "cannot find module `lifecycle`"

**Step 3: Create `packages/core/src/lifecycle/graph.rs`**

```rust
use crate::agent::ActivityState;
use crate::plugins::tracker::TrackerState;
use crate::types::SessionStatus;
use std::collections::HashMap;

/// All data needed to evaluate transitions for one session in one poll tick.
#[derive(Debug, Clone)]
pub struct PollContext {
    pub runtime_alive: bool,
    pub activity_state: ActivityState,
    pub pr: Option<PrState>,
    pub tracker_state: TrackerState,
    pub budget_exceeded: bool,
    pub manual_kill: bool,
}

#[derive(Debug, Clone)]
pub struct PrState {
    pub detected: bool,
    pub state: String,       // "open" | "merged" | "closed"
    pub ci_status: String,   // "pending" | "green" | "failed"
    pub review_decision: String, // "none" | "approved" | "changes_requested"
    pub mergeable: bool,
}

type Guard = fn(&PollContext) -> bool;

struct Edge {
    to: SessionStatus,
    precedence: u32,
    guard: Guard,
}

pub struct StateGraph {
    nodes: HashMap<SessionStatus, Vec<Edge>>,
    terminal: Vec<SessionStatus>,
}

impl StateGraph {
    /// Build and validate the complete state graph from the PRD transition table.
    pub fn build() -> Self {
        let mut nodes: HashMap<SessionStatus, Vec<Edge>> = HashMap::new();

        // Initialize all non-terminal nodes with empty edge lists
        for s in [
            SessionStatus::Spawning,
            SessionStatus::Working,
            SessionStatus::PrOpen,
            SessionStatus::ReviewPending,
            SessionStatus::Approved,
            SessionStatus::Mergeable,
            SessionStatus::CiFailed,
            SessionStatus::ChangesRequested,
            SessionStatus::NeedsInput,
            SessionStatus::Stuck,
        ] {
            nodes.insert(s, vec![]);
        }

        let terminal = vec![
            SessionStatus::Killed,
            SessionStatus::Terminated,
            SessionStatus::Done,
            SessionStatus::Cleanup,
            SessionStatus::Errored,
            SessionStatus::Merged,
        ];

        macro_rules! edge {
            ($from:expr, $to:expr, $prec:expr, $guard:expr) => {
                nodes.entry($from).or_default().push(Edge {
                    to: $to,
                    precedence: $prec,
                    guard: $guard,
                });
            };
        }

        // Local edges (lower precedence number = higher priority)
        edge!(SessionStatus::Spawning,          SessionStatus::Working,          1, |c| c.runtime_alive && c.activity_state == ActivityState::Active);
        edge!(SessionStatus::Spawning,          SessionStatus::Errored,          2, |c| !c.runtime_alive);
        edge!(SessionStatus::Working,           SessionStatus::PrOpen,           3, |c| c.pr.as_ref().map_or(false, |p| p.detected));
        edge!(SessionStatus::Working,           SessionStatus::NeedsInput,       4, |c| c.activity_state == ActivityState::WaitingInput);
        edge!(SessionStatus::Working,           SessionStatus::Stuck,            5, |c| c.activity_state == ActivityState::Idle);
        edge!(SessionStatus::Working,           SessionStatus::Errored,          6, |c| c.activity_state == ActivityState::Blocked);
        edge!(SessionStatus::Working,           SessionStatus::Killed,           7, |c| !c.runtime_alive);
        edge!(SessionStatus::Working,           SessionStatus::Done,             8, |c| c.activity_state == ActivityState::Exited && c.tracker_state == TrackerState::Terminal);
        edge!(SessionStatus::Working,           SessionStatus::Terminated,       9, |c| c.activity_state == ActivityState::Exited);
        edge!(SessionStatus::PrOpen,            SessionStatus::CiFailed,        10, |c| c.pr.as_ref().map_or(false, |p| p.ci_status == "failed"));
        edge!(SessionStatus::PrOpen,            SessionStatus::ReviewPending,   11, |c| c.pr.as_ref().map_or(false, |p| p.ci_status == "green"));
        edge!(SessionStatus::PrOpen,            SessionStatus::Working,         12, |c| c.activity_state == ActivityState::Active);
        edge!(SessionStatus::PrOpen,            SessionStatus::Killed,          13, |c| !c.runtime_alive);
        edge!(SessionStatus::CiFailed,          SessionStatus::Working,         14, |c| c.activity_state == ActivityState::Active);
        edge!(SessionStatus::CiFailed,          SessionStatus::Killed,          15, |c| !c.runtime_alive);
        edge!(SessionStatus::ReviewPending,     SessionStatus::ChangesRequested,16, |c| c.pr.as_ref().map_or(false, |p| p.review_decision == "changes_requested"));
        edge!(SessionStatus::ReviewPending,     SessionStatus::Approved,        17, |c| c.pr.as_ref().map_or(false, |p| p.review_decision == "approved"));
        edge!(SessionStatus::ReviewPending,     SessionStatus::CiFailed,        18, |c| c.pr.as_ref().map_or(false, |p| p.ci_status == "failed"));
        edge!(SessionStatus::ChangesRequested,  SessionStatus::Working,         19, |c| c.activity_state == ActivityState::Active);
        edge!(SessionStatus::ChangesRequested,  SessionStatus::Killed,          20, |c| !c.runtime_alive);
        edge!(SessionStatus::Approved,          SessionStatus::Mergeable,       21, |c| c.pr.as_ref().map_or(false, |p| p.ci_status == "green" && p.mergeable));
        edge!(SessionStatus::Approved,          SessionStatus::CiFailed,        22, |c| c.pr.as_ref().map_or(false, |p| p.ci_status == "failed"));
        edge!(SessionStatus::Mergeable,         SessionStatus::Merged,          23, |c| c.pr.as_ref().map_or(false, |p| p.state == "merged"));
        edge!(SessionStatus::NeedsInput,        SessionStatus::Working,         24, |c| c.activity_state == ActivityState::Active);
        edge!(SessionStatus::NeedsInput,        SessionStatus::Killed,          25, |c| !c.runtime_alive);
        edge!(SessionStatus::Stuck,             SessionStatus::Working,         26, |c| c.activity_state == ActivityState::Active);
        edge!(SessionStatus::Stuck,             SessionStatus::Killed,          27, |c| !c.runtime_alive);

        // Global edges — appended to every non-terminal node
        let global: Vec<(SessionStatus, u32, Guard)> = vec![
            (SessionStatus::Killed,  28, |c: &PollContext| c.manual_kill),
            (SessionStatus::Cleanup, 29, |c: &PollContext| c.tracker_state == TrackerState::Terminal),
            (SessionStatus::Killed,  30, |c: &PollContext| c.budget_exceeded),
        ];

        for (to, prec, guard) in global {
            for edges in nodes.values_mut() {
                edges.push(Edge { to: to.clone(), precedence: prec, guard });
            }
        }

        // Sort each node's edges by precedence (ascending = higher priority first)
        for edges in nodes.values_mut() {
            edges.sort_by_key(|e| e.precedence);
        }

        let graph = Self { nodes, terminal };
        graph.validate();
        graph
    }

    fn validate(&self) {
        // All non-terminal nodes must have at least one outgoing edge
        for (status, edges) in &self.nodes {
            assert!(
                !edges.is_empty(),
                "node {:?} has no outgoing edges",
                status
            );
        }
        // Terminal nodes must NOT be in nodes map (no outgoing edges)
        for t in &self.terminal {
            assert!(
                !self.nodes.contains_key(t),
                "terminal node {:?} should not have outgoing edges",
                t
            );
        }
    }

    /// Evaluate the first matching edge from `current`. Returns None if no guard matches.
    pub fn evaluate(&self, current: SessionStatus, ctx: &PollContext) -> Option<SessionStatus> {
        let edges = self.nodes.get(&current)?;
        for edge in edges {
            if (edge.guard)(ctx) {
                return Some(edge.to.clone());
            }
        }
        None
    }
}
```

**Step 4: Create `packages/core/src/lifecycle/poll.rs`**

This implements the poll tick. MVP gathers only runtime liveness + activity state (PR/CI gather is a stub returning `None`).

```rust
use crate::agent::{Agent, GatherContext};
use crate::lifecycle::graph::{PollContext, PrState, StateGraph};
use crate::plugins::tracker::{TrackerState, Tracker};
use crate::runtime::Runtime;
use crate::session_store::{SessionMetadata, SessionStore, StoreError};
use crate::types::SessionStatus;
use std::sync::Arc;
use tracing::info;

pub struct PollTick<'a> {
    pub graph: &'a StateGraph,
    pub store: &'a SessionStore,
    pub agent: Arc<dyn Agent>,
    pub runtime: Arc<dyn Runtime>,
    pub tracker: Arc<dyn Tracker>,
    pub terminal_states: Vec<String>,
}

impl<'a> PollTick<'a> {
    /// Run one poll tick: gather → evaluate → transition (sequentially).
    pub async fn run(&self) -> Result<Vec<(String, SessionStatus, SessionStatus)>, StoreError> {
        let sessions = self.store.list_active().await?;
        let mut transitions = Vec::new();

        for session in sessions {
            if let Some(new_status) = self.process_session(&session).await {
                transitions.push((session.id.clone(), session.status.clone(), new_status.clone()));
                self.apply_transition(&session, new_status).await;
            }
        }

        Ok(transitions)
    }

    async fn process_session(&self, session: &SessionMetadata) -> Option<SessionStatus> {
        let ctx = self.gather(session).await;
        self.graph.evaluate(session.status.clone(), &ctx)
    }

    async fn gather(&self, session: &SessionMetadata) -> PollContext {
        // 1. Runtime liveness (cheapest — short-circuit if dead)
        let runtime_alive = self.runtime
            .is_alive(&session.id)
            .await
            .unwrap_or(false);

        // 2. Activity state (requires terminal output)
        let activity_state = if runtime_alive {
            let output = self.runtime
                .get_output(&session.id, 50)
                .await
                .unwrap_or_default();
            let aux_path = self.agent.auxiliary_log_path();
            let aux_log = if let Some(ref p) = aux_path {
                tokio::fs::read_to_string(p).await.ok()
            } else {
                None
            };
            let gather_ctx = GatherContext {
                terminal_output: output,
                auxiliary_log: aux_log,
                auxiliary_log_path: aux_path,
            };
            self.agent.detect_activity(&gather_ctx)
        } else {
            crate::agent::ActivityState::Exited
        };

        // 3. PR state — MVP stub (no GitHub API calls yet)
        let pr: Option<PrState> = None;

        // 4. Tracker state — classify from issue state
        let tracker_state = self.gather_tracker_state(session).await;

        PollContext {
            runtime_alive,
            activity_state,
            pr,
            tracker_state,
            budget_exceeded: false,
            manual_kill: session.id.contains("__kill__"), // placeholder; real impl uses a flag file
        }
    }

    async fn gather_tracker_state(&self, session: &SessionMetadata) -> TrackerState {
        match self.tracker.get_issue(&session.issue_url).await {
            Ok(Some(issue)) => {
                let state = issue["state"].as_str().unwrap_or("open");
                crate::plugins::tracker::classify_state(state, &self.terminal_states)
            }
            _ => TrackerState::Active, // fail open
        }
    }

    async fn apply_transition(&self, session: &SessionMetadata, new_status: SessionStatus) {
        info!(
            session_id = %session.id,
            from = %session.status,
            to = %new_status,
            "lifecycle transition"
        );

        let mut updated = session.clone();
        updated.status = new_status.clone();
        updated.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if let Err(e) = self.store.write_metadata(&updated).await {
            tracing::error!(session_id = %session.id, error = %e, "failed to write metadata after transition");
            return;
        }

        // Entry actions for terminal transitions
        match new_status {
            SessionStatus::Killed | SessionStatus::Cleanup => {
                let _ = self.runtime.destroy(&session.id).await;
            }
            _ => {}
        }
    }
}
```

**Step 5: Create `packages/core/src/lifecycle/mod.rs`**

```rust
pub mod graph;
pub mod poll;

pub use graph::{PollContext, PrState, StateGraph};
pub use poll::PollTick;
```

**Step 6: Modify `packages/core/src/lib.rs`**

Add `pub mod lifecycle;` to the list.

**Step 7: Run tests**

```bash
/Users/geraldyeo/.cargo/bin/cargo test -p conductor-core lifecycle 2>&1 | tail -15
```
Expected: all graph tests pass.

**Step 8: Commit**

```bash
git add packages/core/src/lifecycle/ packages/core/src/lib.rs
git commit -m "feat(lifecycle): implement graph-driven state machine and poll tick (ADR-0001)"
```

---

## Phase 2 — Orchestrator Refactor (after Tasks 1–4 merge to main)

---

## Task 5: Orchestrator Refactor

**Prerequisite:** Tasks 1, 2, 3, 4 all merged to `main`. Branch from updated `main`.

**Owns:**
- Modify: `packages/core/src/orchestrator/mod.rs` (complete rewrite)
- Create: `packages/core/src/orchestrator/ipc_server.rs`
- Create: `packages/core/src/orchestrator/spawn_ops.rs`

This is the largest task. The `Orchestrator` struct grows to hold config, per-project stores and plugins, and implements `run()` (IPC listener + poll loop).

**Step 1: Write integration test (IPC round-trip)**

Create `packages/core/tests/orchestrator_ipc.rs`:

```rust
// Integration test: start orchestrator, send Spawn via IPC, verify SpawnResult response.
// Requires: a real git repo (via tempdir), mock agent + runtime.
// This test is marked #[ignore] by default because it binds a real socket.
#[ignore]
#[tokio::test]
async fn test_ipc_spawn_round_trip() {
    // Build an orchestrator with a test config and start it.
    // Send a Spawn request via the IPC client.
    // Assert we get back SpawnResult.
    // This is wired up after the orchestrator implementation is complete.
    todo!()
}
```

**Step 2: Implement `packages/core/src/orchestrator/mod.rs`**

Key structure:
- `Orchestrator` struct holds `Arc<Config>`, `stores: HashMap<String, Arc<SessionStore>>`, `plugins: HashMap<String, ProjectPlugins>`, `graph: Arc<StateGraph>`, socket_path, shutdown channel.
- `Orchestrator::new(config: Config) -> Result<Self, OrchestratorError>` — constructs per-project stores and plugin instances.
- `Orchestrator::run(self) -> Result<(), OrchestratorError>` — binds socket, runs IPC listener + poll loop as concurrent tokio tasks, returns on shutdown.

```rust
use crate::agent::{create_agent, Agent};
use crate::config::{Config, ProjectConfig};
use crate::ipc::{
    read_message, socket_path, write_message, BatchSpawnItem, BatchSpawnOutcome,
    OrchestratorRequest, OrchestratorResponse,
};
use crate::lifecycle::graph::StateGraph;
use crate::lifecycle::poll::PollTick;
use crate::plugins::tracker::classify_state;
use crate::plugins::tracker::{GitHubTracker, TrackerState};
use crate::runtime::{create_runtime, Runtime};
use crate::session_store::{SessionMetadata, SessionStore};
use crate::utils::{CommandRunner, DataPaths};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{error, info, warn};

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("config error: {0}")]
    Config(#[from] crate::config::ConfigError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("orchestrator already running (socket exists at {0:?})")]
    AlreadyRunning(PathBuf),
    #[error("project not found: {0}")]
    ProjectNotFound(String),
    #[error("issue is in terminal state")]
    IssueTerminal,
    #[error("issue not found: {0}")]
    IssueNotFound(String),
    #[error("session already exists: {0}")]
    SessionExists(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("workspace error: {0}")]
    WorkspaceError(String),
    #[error("runtime error: {0}")]
    RuntimeError(String),
    #[error("tracker error: {0}")]
    TrackerError(String),
    #[error("data paths error: {0}")]
    DataPaths(#[from] crate::utils::DataPathsError),
    #[error("store error: {0}")]
    Store(#[from] crate::session_store::StoreError),
}

struct ProjectPlugins {
    agent: Arc<dyn Agent>,
    runtime: Arc<dyn Runtime>,
    store: Arc<SessionStore>,
    terminal_states: Vec<String>,
    repo_root: PathBuf,
}

type IpcMsg = (OrchestratorRequest, oneshot::Sender<OrchestratorResponse>);

pub struct Orchestrator {
    config: Arc<Config>,
    plugins: HashMap<String, ProjectPlugins>,
    graph: Arc<StateGraph>,
    socket_path: PathBuf,
}

impl Orchestrator {
    pub async fn new(config: Config) -> Result<Self, OrchestratorError> {
        let config = Arc::new(config);
        let mut plugins = HashMap::new();

        for (project_id, project) in &config.projects {
            let runner = CommandRunner::default();
            let agent_name = project.agent.as_deref().unwrap_or("claude-code");
            let runtime_name = project.runtime.as_deref().unwrap_or("tmux");

            let agent = create_agent(agent_name)
                .map_err(|e| OrchestratorError::RuntimeError(e.to_string()))?;
            let runtime = create_runtime(runtime_name, runner.clone())
                .map_err(|e| OrchestratorError::RuntimeError(e.to_string()))?;

            let config_path = std::env::current_dir()?.join("agent-orchestrator.yaml");
            let paths = DataPaths::new(&config_path, project_id);
            paths.ensure_dirs().await?;

            let store = Arc::new(SessionStore::new(paths));
            let terminal_states = project.tracker.terminal_states.clone();
            let repo_root = PathBuf::from(shellexpand::tilde(&project.path).as_ref());

            plugins.insert(
                project_id.clone(),
                ProjectPlugins {
                    agent: Arc::from(agent),
                    runtime: Arc::from(runtime),
                    store,
                    terminal_states,
                    repo_root,
                },
            );
        }

        let graph = Arc::new(StateGraph::build());
        let socket_path = socket_path();

        Ok(Self { config, plugins, graph, socket_path })
    }

    /// Run the orchestrator: IPC listener + poll loop.
    /// Blocks until shutdown signal.
    pub async fn run(self) -> Result<(), OrchestratorError> {
        if self.socket_path.exists() {
            // Attempt connection — if refused it's stale
            match tokio::net::UnixStream::connect(&self.socket_path).await {
                Ok(_) => return Err(OrchestratorError::AlreadyRunning(self.socket_path)),
                Err(_) => {
                    tokio::fs::remove_file(&self.socket_path).await.ok();
                }
            }
        }

        let listener = UnixListener::bind(&self.socket_path)?;
        info!(socket = ?self.socket_path, "orchestrator listening");

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (ipc_tx, mut ipc_rx) = mpsc::channel::<IpcMsg>(64);

        // Spawn IPC listener task
        let ipc_tx2 = ipc_tx.clone();
        let mut shutdown_rx2 = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _)) => {
                                let tx = ipc_tx2.clone();
                                tokio::spawn(handle_connection(stream, tx));
                            }
                            Err(e) => {
                                error!(error = %e, "accept error");
                                break;
                            }
                        }
                    }
                    _ = shutdown_rx2.changed() => break,
                }
            }
        });

        // Ctrl-C / SIGTERM handler
        let shutdown_tx2 = shutdown_tx.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("shutdown signal received");
            let _ = shutdown_tx2.send(true);
        });

        // Main event loop: drain IPC channel between poll ticks
        let poll_interval = tokio::time::Duration::from_secs(30);
        let mut interval = tokio::time::interval(poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.poll_tick().await;
                }
                Some((req, reply_tx)) = ipc_rx.recv() => {
                    let response = self.handle_request(req).await;
                    let _ = reply_tx.send(response);
                }
                _ = shutdown_rx.changed() => {
                    info!("orchestrator shutting down");
                    break;
                }
            }
        }

        tokio::fs::remove_file(&self.socket_path).await.ok();
        Ok(())
    }

    async fn poll_tick(&self) {
        for (project_id, p) in &self.plugins {
            // MVP: stub tracker — use a no-op tracker for poll
            // Real tracker wiring is post-MVP (would require async factory)
            let tick = PollTick {
                graph: &self.graph,
                store: &p.store,
                agent: p.agent.clone(),
                runtime: p.runtime.clone(),
                tracker: Arc::new(NoOpTracker),
                terminal_states: p.terminal_states.clone(),
            };
            match tick.run().await {
                Ok(transitions) => {
                    for (id, from, to) in transitions {
                        info!(session_id = %id, %from, %to, project = %project_id, "poll transition");
                    }
                }
                Err(e) => {
                    error!(project = %project_id, error = %e, "poll tick error");
                }
            }
        }
    }

    async fn handle_request(&self, req: OrchestratorRequest) -> OrchestratorResponse {
        match req {
            OrchestratorRequest::Spawn { project_id, issue_url, agent: _, open: _ } => {
                match self.handle_spawn(&project_id, &issue_url).await {
                    Ok((session_id, branch, workspace_path)) => OrchestratorResponse::SpawnResult {
                        session_id,
                        branch,
                        workspace_path: workspace_path.to_string_lossy().to_string(),
                    },
                    Err(e) => error_response(e),
                }
            }
            OrchestratorRequest::Stop => {
                // Handled by the run() loop via shutdown channel; this branch is for
                // explicit IPC Stop requests (ao stop sends this).
                OrchestratorResponse::Ok { message: "stopping".to_string() }
            }
            OrchestratorRequest::Kill { session_id } => {
                match self.handle_kill(&session_id).await {
                    Ok(_) => OrchestratorResponse::Ok { message: format!("kill scheduled for {session_id}") },
                    Err(e) => error_response(e),
                }
            }
            OrchestratorRequest::Cleanup { project_id, dry_run } => {
                match self.handle_cleanup(&project_id, dry_run).await {
                    Ok((killed, skipped)) => OrchestratorResponse::CleanupResult { killed, skipped },
                    Err(e) => error_response(e),
                }
            }
            OrchestratorRequest::BatchSpawn { project_id, issue_urls, agent: _, open: _ } => {
                let results = self.handle_batch_spawn(&project_id, issue_urls).await;
                OrchestratorResponse::BatchSpawnResult { results }
            }
            OrchestratorRequest::Send { session_id, content, no_wait: _, timeout_secs: _ } => {
                match self.handle_send(&session_id, &content).await {
                    Ok(_) => OrchestratorResponse::SendResult {
                        delivered: true,
                        activity_state: "unknown".to_string(),
                    },
                    Err(e) => error_response(e),
                }
            }
        }
    }

    async fn handle_spawn(
        &self,
        project_id: &str,
        issue_url: &str,
    ) -> Result<(String, String, PathBuf), OrchestratorError> {
        let p = self.plugins.get(project_id)
            .ok_or_else(|| OrchestratorError::ProjectNotFound(project_id.to_string()))?;

        let project = self.config.projects.get(project_id).unwrap();

        // Step 1: Pre-spawn tracker validation
        let runner = CommandRunner::default();
        let tracker = GitHubTracker::new(
            runner.clone(),
            project.repo.clone(),
            p.terminal_states.clone(),
        ).await.map_err(|e| OrchestratorError::TrackerError(e.to_string()))?;

        let issue = tracker.get_issue(issue_url).await
            .map_err(|e| OrchestratorError::TrackerError(e.to_string()))?
            .ok_or_else(|| OrchestratorError::IssueNotFound(issue_url.to_string()))?;

        let state = issue["state"].as_str().unwrap_or("open");
        if classify_state(state, &p.terminal_states) == TrackerState::Terminal {
            return Err(OrchestratorError::IssueTerminal);
        }

        let issue_number = issue["number"].as_u64().unwrap_or(0);
        let title = issue["title"].as_str().unwrap_or("");
        let branch = tracker.branch_name(issue_number, title);

        // Step 2: Session ID
        let session_id = make_session_id(issue_url);

        // Step 3: Create session record
        p.store.paths_ref().ensure_dirs().await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let metadata = SessionMetadata {
            id: session_id.clone(),
            status: crate::types::SessionStatus::Spawning,
            termination_reason: None,
            issue_url: issue_url.to_string(),
            branch: branch.clone(),
            worktree_path: p.store.paths_ref().worktree_path(&session_id),
            created_at: now,
            updated_at: now,
            attempts: 1,
            total_cost_usd: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            project_id: project_id.to_string(),
            agent: project.agent.as_deref().unwrap_or("claude-code").to_string(),
            runtime: project.runtime.as_deref().unwrap_or("tmux").to_string(),
            workspace: project.workspace.as_deref().unwrap_or("worktree").to_string(),
            tracker: project.tracker.plugin.clone(),
        };

        p.store.create_session(&metadata).await?;

        // Step 4: Create workspace (git worktree)
        let worktree_path = p.store.paths_ref().worktree_path(&session_id);
        let out = runner.run_in_dir(
            &["git", "worktree", "add", &worktree_path.to_string_lossy(), "-b", &branch],
            &p.repo_root,
            None,
            None,
        ).await.map_err(|e| OrchestratorError::WorkspaceError(e.to_string()))?;

        if !out.success {
            let _ = p.store.delete_session(&session_id).await;
            return Err(OrchestratorError::WorkspaceError(out.stderr));
        }

        // Step 5: Get issue content + render prompt
        let issue_content = tracker.get_issue_content(issue_url).await
            .map_err(|e| OrchestratorError::TrackerError(e.to_string()))?;

        let prompt_engine = crate::prompt::PromptEngine::new();
        let prompt = prompt_engine.render_launch(
            issue_url,
            &worktree_path,
            &issue_content,
            project.agent_rules.as_deref(),
            &[],
        ).await.unwrap_or_else(|_| "Complete the GitHub issue.".to_string());

        // Step 6: Build and execute LaunchPlan
        let launch_ctx = crate::agent::LaunchContext {
            session_id: session_id.clone(),
            prompt,
            workspace_path: worktree_path.clone(),
            issue_id: issue_url.to_string(),
            branch: branch.clone(),
        };

        let plan = p.agent.launch_plan(&launch_ctx);
        for step in &plan.steps {
            p.runtime.execute_step(&session_id, step).await
                .map_err(|e| OrchestratorError::RuntimeError(e.to_string()))?;
        }

        // Update status to Working
        let mut updated = metadata;
        updated.status = crate::types::SessionStatus::Working;
        updated.updated_at = now;
        p.store.write_metadata(&updated).await?;

        Ok((session_id, branch, worktree_path))
    }

    async fn handle_kill(&self, session_id: &str) -> Result<(), OrchestratorError> {
        // Find the session across all projects
        for p in self.plugins.values() {
            if let Ok(mut meta) = p.store.read_metadata(session_id).await {
                meta.status = crate::types::SessionStatus::Killed;
                meta.updated_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                p.store.write_metadata(&meta).await?;
                let _ = p.runtime.destroy(session_id).await;
                return Ok(());
            }
        }
        Err(OrchestratorError::SessionNotFound(session_id.to_string()))
    }

    async fn handle_cleanup(&self, project_id: &str, dry_run: bool) -> Result<(Vec<String>, Vec<String>), OrchestratorError> {
        let p = self.plugins.get(project_id)
            .ok_or_else(|| OrchestratorError::ProjectNotFound(project_id.to_string()))?;
        let project = self.config.projects.get(project_id).unwrap();

        let runner = CommandRunner::default();
        let tracker = GitHubTracker::new(runner, project.repo.clone(), p.terminal_states.clone())
            .await
            .map_err(|e| OrchestratorError::TrackerError(e.to_string()))?;

        let active = p.store.list_active().await?;
        let mut killed = Vec::new();
        let mut skipped = Vec::new();

        for session in active {
            match tracker.get_issue(&session.issue_url).await {
                Ok(Some(issue)) => {
                    let state = issue["state"].as_str().unwrap_or("open");
                    if classify_state(state, &p.terminal_states) == TrackerState::Terminal {
                        if !dry_run {
                            let mut updated = session.clone();
                            updated.status = crate::types::SessionStatus::Killed;
                            let _ = p.store.write_metadata(&updated).await;
                            let _ = p.runtime.destroy(&session.id).await;
                        }
                        killed.push(session.id);
                    } else {
                        skipped.push(session.id);
                    }
                }
                _ => skipped.push(session.id),
            }
        }

        Ok((killed, skipped))
    }

    async fn handle_batch_spawn(&self, project_id: &str, issue_urls: Vec<String>) -> Vec<BatchSpawnItem> {
        let mut results = Vec::new();
        for issue_url in issue_urls {
            let outcome = match self.handle_spawn(project_id, &issue_url).await {
                Ok((session_id, branch, _)) => BatchSpawnOutcome::Spawned { session_id, branch },
                Err(OrchestratorError::SessionExists(id)) => {
                    BatchSpawnOutcome::Skipped { reason: format!("session {id} already exists") }
                }
                Err(OrchestratorError::IssueTerminal) => {
                    BatchSpawnOutcome::Skipped { reason: "issue is terminal".to_string() }
                }
                Err(e) => BatchSpawnOutcome::Failed { error: e.to_string() },
            };
            results.push(BatchSpawnItem { issue_url, outcome });
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
        results
    }

    async fn handle_send(&self, session_id: &str, content: &str) -> Result<(), OrchestratorError> {
        use crate::types::RuntimeStep;
        for p in self.plugins.values() {
            if p.store.read_metadata(session_id).await.is_ok() {
                p.runtime.execute_step(session_id, &RuntimeStep::SendMessage {
                    content: content.to_string(),
                }).await.map_err(|e| OrchestratorError::RuntimeError(e.to_string()))?;
                return Ok(());
            }
        }
        Err(OrchestratorError::SessionNotFound(session_id.to_string()))
    }
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    tx: mpsc::Sender<IpcMsg>,
) {
    let request: OrchestratorRequest = match read_message(&mut stream).await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "failed to read IPC request");
            return;
        }
    };
    let (reply_tx, reply_rx) = oneshot::channel();
    let _ = tx.send((request, reply_tx)).await;
    if let Ok(response) = reply_rx.await {
        let _ = write_message(&mut stream, &response).await;
    }
}

fn error_response(e: OrchestratorError) -> OrchestratorResponse {
    let code = match &e {
        OrchestratorError::IssueTerminal => "issue_terminal",
        OrchestratorError::IssueNotFound(_) => "issue_not_found",
        OrchestratorError::SessionExists(_) => "session_exists",
        OrchestratorError::SessionNotFound(_) => "session_not_found",
        OrchestratorError::ProjectNotFound(_) => "project_not_found",
        _ => "internal_error",
    };
    OrchestratorResponse::Error { code: code.to_string(), message: e.to_string() }
}

fn make_session_id(issue_url: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in issue_url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("ao-{:x}", hash)
}

/// No-op tracker for poll tick (real tracker wiring is post-MVP).
struct NoOpTracker;

#[async_trait::async_trait]
impl crate::plugins::tracker::Tracker for NoOpTracker {
    async fn get_issue(&self, _: &str) -> Result<Option<serde_json::Value>, crate::plugins::tracker::TrackerError> {
        Ok(None)
    }
    fn branch_name(&self, n: u64, _: &str) -> String { format!("ao/issue-{n}") }
    fn issue_url(&self, n: u64) -> String { format!("#{n}") }
    async fn get_issue_content(&self, _: &str) -> Result<crate::plugins::tracker::IssueContent, crate::plugins::tracker::TrackerError> {
        Err(crate::plugins::tracker::TrackerError::NotFound("noop".into()))
    }
    async fn add_comment(&self, _: &str, _: &str) -> Result<(), crate::plugins::tracker::TrackerError> {
        Ok(())
    }
}
```

**Note:** `SessionStore` needs a `paths_ref()` method and a `delete_session()` method. Add these to `session_store/mod.rs`:
```rust
// In impl SessionStore:
pub fn paths_ref(&self) -> &DataPaths { &self.paths }

pub async fn delete_session(&self, session_id: &str) -> Result<(), StoreError> {
    let dir = self.paths.session_dir(session_id);
    tokio::fs::remove_dir_all(&dir).await.map_err(StoreError::Io)
}
```

Also, `create_runtime()` factory needs to exist. Add to `packages/core/src/runtime/mod.rs`:
```rust
pub fn create_runtime(name: &str, runner: crate::utils::CommandRunner) -> Result<Box<dyn Runtime>, crate::types::PluginError> {
    match name {
        "tmux" => Ok(Box::new(TmuxRuntime::new(Arc::new(runner)))),
        other => Err(crate::types::PluginError::NotImplemented(other.into())),
    }
}
```

And `PromptEngine::render_launch()` — check what the current signature is and adjust the call accordingly. See `packages/core/src/prompt/mod.rs` for the real signature.

**Step 3: Run build**

```bash
/Users/geraldyeo/.cargo/bin/cargo build -p conductor-core 2>&1 | tail -20
```

Fix any compilation errors. Key things to verify:
- `SessionStore` exposes `paths_ref()`
- `create_runtime()` is exported from `runtime/mod.rs`
- `PromptEngine::render_launch()` signature matches

**Step 4: Run tests**

```bash
/Users/geraldyeo/.cargo/bin/cargo test -p conductor-core 2>&1 | tail -20
```
Expected: all existing tests still pass.

**Step 5: Commit**

```bash
git add packages/core/src/orchestrator/ packages/core/src/session_store/mod.rs packages/core/src/runtime/mod.rs
git commit -m "feat(orchestrator): full daemon with IPC server, poll loop, and all session operations"
```

---

## Phase 3 — CLI Commands (after Task 5 merges)

---

## Task 6: CLI Commands + Output Module

**Prerequisite:** Task 5 merged to `main`. Branch from updated `main`.

**Owns:**
- Rewrite: `packages/cli/src/main.rs`
- Create: `packages/cli/src/commands/` (init, start, stop, status, spawn, batch_spawn, send, session)
- Create: `packages/cli/src/output/` (mod, table, json)
- Create: `packages/cli/src/resolve.rs`
- Create: `packages/cli/src/error.rs`
- Delete: `packages/cli/src/spawn.rs` (replaced by commands/spawn.rs)
- Modify: `packages/cli/Cargo.toml` (add `conductor-core` config feature usage, `shellexpand`)

**Step 1: Create error module `packages/cli/src/error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    General(String),
    #[error("config error: {0}")]
    Config(#[from] conductor_core::config::ConfigError),
    #[error("{0}")]
    Ipc(#[from] crate::ipc::client::IpcError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Map CliError to process exit code.
pub fn exit_code(e: &CliError) -> i32 {
    match e {
        CliError::Config(_) => 3,
        CliError::Ipc(crate::ipc::client::IpcError::NotRunning(_)) => 4,
        CliError::General(_) | CliError::Io(_) | CliError::Ipc(_) => 1,
    }
}

pub fn print_error(e: &CliError, json: bool) {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({"error": {"code": "error", "message": e.to_string()}})
        );
    } else {
        eprintln!("error: {e}");
    }
}
```

**Step 2: Create `packages/cli/src/resolve.rs`**

```rust
use conductor_core::config::Config;
use thiserror::Error;
use std::path::Path;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("project '{0}' not found in config")]
    NotFound(String),
    #[error("multiple projects available, specify with -p: {0}")]
    Ambiguous(String),
}

/// Resolve the active project ID from CLI flags + config + CWD.
pub fn resolve_project<'a>(
    config: &'a Config,
    flag: Option<&str>,
) -> Result<&'a str, ResolveError> {
    // 1. Explicit flag
    if let Some(id) = flag {
        if config.projects.contains_key(id) {
            return Ok(id);
        }
        return Err(ResolveError::NotFound(id.to_string()));
    }

    // 2. Single project
    if config.projects.len() == 1 {
        return Ok(config.projects.keys().next().unwrap());
    }

    // 3. CWD inside a project's path
    if let Ok(cwd) = std::env::current_dir() {
        for (id, project) in &config.projects {
            let project_path = Path::new(&shellexpand::tilde(&project.path).as_ref().to_string());
            if cwd.starts_with(project_path) {
                return Ok(id);
            }
        }
    }

    // 4. Ambiguous
    let names: Vec<_> = config.projects.keys().cloned().collect();
    Err(ResolveError::Ambiguous(names.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use conductor_core::config::{Config, ProjectConfig, Defaults, TrackerConfig, Hooks};
    use std::collections::HashMap;

    fn make_config(projects: Vec<(&str, &str)>) -> Config {
        // Build a minimal Config for testing resolve logic.
        // This helper skips validation (path may not exist in tests).
        // We use serde_json round-trip as a workaround.
        let mut map = HashMap::new();
        for (id, path) in projects {
            map.insert(id.to_string(), serde_json::json!({
                "repo": "owner/repo",
                "path": path,
            }));
        }
        let value = serde_json::json!({ "projects": map });
        serde_json::from_value(value).expect("invalid test config")
    }

    #[test]
    fn test_explicit_flag_found() {
        let config = make_config(vec![("myapp", "/tmp"), ("other", "/tmp")]);
        let result = resolve_project(&config, Some("myapp")).unwrap();
        assert_eq!(result, "myapp");
    }

    #[test]
    fn test_explicit_flag_not_found() {
        let config = make_config(vec![("myapp", "/tmp")]);
        assert!(matches!(resolve_project(&config, Some("other")), Err(ResolveError::NotFound(_))));
    }

    #[test]
    fn test_single_project_auto_resolved() {
        let config = make_config(vec![("myapp", "/tmp")]);
        let result = resolve_project(&config, None).unwrap();
        assert_eq!(result, "myapp");
    }

    #[test]
    fn test_multi_project_ambiguous() {
        let config = make_config(vec![("a", "/noexist1"), ("b", "/noexist2")]);
        assert!(matches!(resolve_project(&config, None), Err(ResolveError::Ambiguous(_))));
    }
}
```

**Step 3: Create `packages/cli/src/output/mod.rs`**

```rust
pub mod table;
pub mod json;

#[derive(Debug, Clone, Copy)]
pub enum OutputMode { Human, Json }
```

**Step 4: Create `packages/cli/src/output/table.rs`**

```rust
use conductor_core::session_store::SessionMetadata;

pub fn format_sessions(sessions: &[SessionMetadata]) -> String {
    if sessions.is_empty() {
        return "No sessions.".to_string();
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let header = format!(
        "{:<25} {:<18} {:<8} {:<28} {:<8} {}",
        "SESSION", "STATUS", "ISSUE", "BRANCH", "AGE", "TOKENS"
    );
    let sep = "-".repeat(header.len());

    let mut rows = vec![header, sep];
    for s in sessions {
        let age = format_age(now.saturating_sub(s.created_at));
        let tokens = format!(
            "{} in / {} out",
            format_tokens(s.input_tokens),
            format_tokens(s.output_tokens)
        );
        let issue = extract_issue_number(&s.issue_url)
            .map(|n| format!("#{n}"))
            .unwrap_or_else(|| s.issue_url.clone());
        rows.push(format!(
            "{:<25} {:<18} {:<8} {:<28} {:<8} {}",
            s.id, s.status, issue, s.branch, age, tokens
        ));
    }
    rows.join("\n")
}

fn format_age(secs: u64) -> String {
    if secs < 60 { format!("{secs}s") }
    else if secs < 3600 { format!("{}m", secs / 60) }
    else { format!("{}h", secs / 3600) }
}

fn format_tokens(n: u64) -> String {
    if n >= 1000 { format!("{:.1}k", n as f64 / 1000.0) }
    else { n.to_string() }
}

fn extract_issue_number(url: &str) -> Option<u64> {
    conductor_core::utils::parse_github_issue_number(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1.0k");
        assert_eq!(format_tokens(12400), "12.4k");
    }

    #[test]
    fn test_format_age() {
        assert_eq!(format_age(30), "30s");
        assert_eq!(format_age(90), "1m");
        assert_eq!(format_age(7200), "2h");
    }
}
```

**Step 5: Rewrite `packages/cli/src/main.rs`**

```rust
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
        Commands::Init { auto, output } => {
            commands::init::run(auto, output, cli.json).await
        }
        Commands::Start { .. } => {
            commands::start::run(cli.project.as_deref(), cli.config.as_deref(), cli.json).await
        }
        Commands::Stop => commands::stop::run(cli.json).await,
        Commands::Status => {
            commands::status::run(cli.project.as_deref(), cli.config.as_deref(), cli.json).await
        }
        Commands::Spawn { issue_url, agent, open } => {
            commands::spawn::run(&issue_url, agent.as_deref(), open, cli.project.as_deref(), cli.config.as_deref(), cli.json).await
        }
        Commands::BatchSpawn { issue_urls, agent } => {
            commands::batch_spawn::run(issue_urls, agent.as_deref(), cli.project.as_deref(), cli.config.as_deref(), cli.json).await
        }
        Commands::Send { session_id, message, file, no_wait, timeout } => {
            let content = build_send_content(message, file).await?;
            commands::send::run(&session_id, &content, no_wait, timeout, cli.json).await
        }
        Commands::Session { cmd } => match cmd {
            SessionCmd::Ls => {
                commands::status::run(cli.project.as_deref(), cli.config.as_deref(), cli.json).await
            }
            SessionCmd::Kill { session_id } => {
                commands::session::kill(&session_id, cli.json).await
            }
            SessionCmd::Cleanup { dry_run } => {
                commands::session::cleanup(dry_run, cli.project.as_deref(), cli.config.as_deref(), cli.json).await
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
    tokio::io::stdin().read_to_string(&mut buf).await.map_err(CliError::Io)?;
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
```

**Step 6: Create command modules**

Create `packages/cli/src/commands/mod.rs`:
```rust
pub mod batch_spawn;
pub mod init;
pub mod send;
pub mod session;
pub mod spawn;
pub mod start;
pub mod status;
pub mod stop;
```

Create `packages/cli/src/commands/init.rs`:
```rust
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
        println!("{}", serde_json::json!({"created": out_path.to_string_lossy()}));
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
    // Handle SSH: git@github.com:owner/repo.git → owner/repo
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return Some(rest.trim_end_matches(".git").to_string());
    }
    // Handle HTTPS: https://github.com/owner/repo.git → owner/repo
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        return Some(rest.trim_end_matches(".git").to_string());
    }
    None
}

fn prompt_interactively() -> Result<(String, String, String), CliError> {
    // MVP: --auto only; interactive prompts are post-MVP
    Err(CliError::General(
        "Interactive mode not yet implemented. Use --auto flag.".to_string()
    ))
}
```

Create `packages/cli/src/commands/start.rs`:
```rust
use crate::error::CliError;
use conductor_core::orchestrator::Orchestrator;
use std::path::Path;

pub async fn run(project: Option<&str>, config_path: Option<&Path>, json: bool) -> Result<(), CliError> {
    let config = load_config(config_path)?;

    if json {
        eprintln!("{}", serde_json::json!({"status": "starting"}));
    } else {
        println!("Starting orchestrator. Press Ctrl-C to stop.");
    }

    let orchestrator = Orchestrator::new(config).await
        .map_err(|e| CliError::General(e.to_string()))?;

    orchestrator.run().await
        .map_err(|e| CliError::General(e.to_string()))?;

    Ok(())
}

fn load_config(path: Option<&Path>) -> Result<conductor_core::config::Config, CliError> {
    if let Some(p) = path {
        conductor_core::config::load_from_path(p).map_err(CliError::Config)
    } else {
        conductor_core::config::load().map_err(CliError::Config)
    }
}
```

Create `packages/cli/src/commands/stop.rs`:
```rust
use crate::error::CliError;
use crate::ipc::client::send_request;
use conductor_core::ipc::OrchestratorRequest;

pub async fn run(json: bool) -> Result<(), CliError> {
    match send_request(&OrchestratorRequest::Stop).await {
        Ok(_) => {
            if json {
                println!("{}", serde_json::json!({"status": "stopped"}));
            } else {
                println!("Orchestrator stopped.");
            }
        }
        Err(crate::ipc::client::IpcError::NotRunning(_)) => {
            if !json {
                println!("Orchestrator is not running.");
            }
        }
        Err(e) => return Err(CliError::Ipc(e)),
    }
    Ok(())
}
```

Create `packages/cli/src/commands/status.rs`:
```rust
use crate::error::CliError;
use crate::output::table::format_sessions;
use conductor_core::utils::DataPaths;
use std::path::Path;

pub async fn run(project: Option<&str>, config_path: Option<&Path>, json: bool) -> Result<(), CliError> {
    let config = if let Some(p) = config_path {
        conductor_core::config::load_from_path(p)?
    } else {
        conductor_core::config::load()?
    };

    let config_file = config_path
        .map(|p| p.to_path_buf())
        .or_else(|| std::env::current_dir().ok().map(|d| d.join("agent-orchestrator.yaml")))
        .unwrap_or_else(|| std::path::PathBuf::from("agent-orchestrator.yaml"));

    let mut all_sessions = Vec::new();

    for (project_id, _) in &config.projects {
        if let Some(p) = project {
            if project_id != p {
                continue;
            }
        }
        let paths = DataPaths::new(&config_file, project_id);
        let store = conductor_core::session_store::SessionStore::new(paths);
        let sessions = store.list().await.map_err(|e| CliError::General(e.to_string()))?;
        all_sessions.extend(sessions);
    }

    if json {
        println!("{}", serde_json::to_string(&all_sessions.iter().map(|s| {
            serde_json::json!({
                "id": s.id,
                "status": s.status.to_string(),
                "issue_url": s.issue_url,
                "branch": s.branch,
                "created_at": s.created_at,
                "input_tokens": s.input_tokens,
                "output_tokens": s.output_tokens,
            })
        }).collect::<Vec<_>>()).unwrap());
    } else {
        println!("{}", format_sessions(&all_sessions));
    }

    Ok(())
}
```

Create `packages/cli/src/commands/spawn.rs`:
```rust
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

    let project_id = resolve_project(&config, project)
        .map_err(|e| CliError::General(e.to_string()))?;

    let req = OrchestratorRequest::Spawn {
        project_id: project_id.to_string(),
        issue_url: issue_url.to_string(),
        agent: agent.map(String::from),
        open,
    };

    let response = send_request(&req).await?;

    match response {
        OrchestratorResponse::SpawnResult { session_id, branch, workspace_path } => {
            if json {
                println!("{}", serde_json::json!({
                    "session_id": session_id,
                    "branch": branch,
                    "workspace_path": workspace_path,
                }));
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
```

Create `packages/cli/src/commands/batch_spawn.rs`:
```rust
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

    let project_id = resolve_project(&config, project)
        .map_err(|e| CliError::General(e.to_string()))?;

    let req = OrchestratorRequest::BatchSpawn {
        project_id: project_id.to_string(),
        issue_urls,
        agent: agent.map(String::from),
        open: false,
    };

    let response = send_request(&req).await?;

    if let OrchestratorResponse::BatchSpawnResult { results } = response {
        if json {
            println!("{}", serde_json::to_string(&results).unwrap());
        } else {
            for item in &results {
                match &item.outcome {
                    BatchSpawnOutcome::Spawned { session_id, branch } => {
                        println!("spawned  {} → {} ({})", item.issue_url, session_id, branch);
                    }
                    BatchSpawnOutcome::Skipped { reason } => {
                        println!("skipped  {} — {}", item.issue_url, reason);
                    }
                    BatchSpawnOutcome::Failed { error } => {
                        eprintln!("failed   {} — {}", item.issue_url, error);
                    }
                }
            }
        }
    }

    Ok(())
}
```

Create `packages/cli/src/commands/send.rs`:
```rust
use crate::error::CliError;
use crate::ipc::client::send_request;
use conductor_core::ipc::{OrchestratorRequest, OrchestratorResponse};
use conductor_core::utils::CommandRunner;

pub async fn run(
    session_id: &str,
    content: &str,
    no_wait: bool,
    timeout: u64,
    json: bool,
) -> Result<(), CliError> {
    let req = OrchestratorRequest::Send {
        session_id: session_id.to_string(),
        content: content.to_string(),
        no_wait,
        timeout_secs: timeout,
    };

    match send_request(&req).await {
        Ok(OrchestratorResponse::SendResult { delivered, activity_state }) => {
            if json {
                println!("{}", serde_json::json!({"delivered": delivered, "activity_state": activity_state}));
            } else {
                println!("Delivered: {delivered} (agent state: {activity_state})");
            }
        }
        Err(crate::ipc::client::IpcError::NotRunning(_)) => {
            // Fallback: direct tmux send-keys
            eprintln!("warning: orchestrator not running, delivering without busy detection");
            let runner = CommandRunner::default();
            runner.run(&["tmux", "send-keys", "-t", session_id, content, "Enter"], None, None)
                .await
                .map_err(|e| CliError::General(e.to_string()))?;
            if json {
                println!("{}", serde_json::json!({"delivered": true, "activity_state": "unknown", "fallback": true}));
            } else {
                println!("Delivered via direct tmux send-keys.");
            }
        }
        Err(e) => return Err(CliError::Ipc(e)),
        Ok(other) => return Err(CliError::General(format!("unexpected response: {other:?}"))),
    }

    Ok(())
}
```

Create `packages/cli/src/commands/session.rs`:
```rust
use crate::error::CliError;
use crate::ipc::client::send_request;
use crate::resolve::resolve_project;
use conductor_core::ipc::{OrchestratorRequest, OrchestratorResponse};
use std::path::Path;

pub async fn kill(session_id: &str, json: bool) -> Result<(), CliError> {
    let req = OrchestratorRequest::Kill { session_id: session_id.to_string() };
    let response = send_request(&req).await?;

    if json {
        println!("{}", serde_json::json!({"session_id": session_id, "status": "kill_scheduled"}));
    } else {
        // Poll SessionStore for up to 5s to see if it transitions
        let _ = wait_for_killed(session_id).await;
        println!("Kill scheduled for {session_id}.");
    }
    Ok(())
}

async fn wait_for_killed(session_id: &str) {
    // Brief polling — best-effort, not required for correctness
    // (actual kill happens in next poll tick)
}

pub async fn cleanup(dry_run: bool, project: Option<&str>, config_path: Option<&Path>, json: bool) -> Result<(), CliError> {
    let config = if let Some(p) = config_path {
        conductor_core::config::load_from_path(p)?
    } else {
        conductor_core::config::load()?
    };

    let project_id = resolve_project(&config, project)
        .map_err(|e| CliError::General(e.to_string()))?;

    let req = OrchestratorRequest::Cleanup {
        project_id: project_id.to_string(),
        dry_run,
    };

    let response = send_request(&req).await?;

    if let OrchestratorResponse::CleanupResult { killed, skipped } = response {
        if json {
            println!("{}", serde_json::json!({"killed": killed, "skipped": skipped}));
        } else {
            println!("Killed: {}", killed.join(", "));
            println!("Skipped: {}", skipped.join(", "));
        }
    }

    Ok(())
}
```

**Step 7: Delete old spawn.rs**

```bash
rm packages/cli/src/spawn.rs
```

**Step 8: Add `shellexpand` to CLI Cargo.toml**

```toml
shellexpand = { workspace = true }
```

**Step 9: Run build**

```bash
/Users/geraldyeo/.cargo/bin/cargo build -p cli 2>&1 | tail -20
```

Fix any compilation errors. Common issues:
- `SessionMetadata` doesn't implement `Serialize` — add `#[derive(Serialize)]` or use the manual conversion in status.rs json output.
- Missing `pub use` for some types.

**Step 10: Run all tests**

```bash
/Users/geraldyeo/.cargo/bin/cargo test --workspace 2>&1 | tail -20
```
Expected: all tests pass.

**Step 11: Run clippy**

```bash
/Users/geraldyeo/.cargo/bin/cargo clippy --workspace -- -D warnings 2>&1 | tail -20
```
Fix all warnings.

**Step 12: Manual smoke test**

```bash
# In a repo with agent-orchestrator.yaml:
ao init --auto
# edit the generated file to set real values
ao start &
ao status
ao stop
```

**Step 13: Commit**

```bash
git add packages/cli/src/ && git rm packages/cli/src/spawn.rs
git commit -m "feat(cli): implement all 9 MVP commands (init, start, stop, status, spawn, batch-spawn, send, session ls/kill/cleanup)"
```

---

## Execution Handoff

After each task's PR is opened, run `/code-review-multi diff` in the worktree. Fix Criticals before opening PR. Document unresolved Warnings in the PR description. Gemini Code Assist will run automatically — address Critical/High, reply to intentional-design Mediums.

**Merge order:**
1. Tasks 1, 2, 3 in parallel (no dependency between them)
2. Task 4 (after Task 1 merges — shares `lib.rs`)
3. Task 5 (after Tasks 1–4 merge)
4. Task 6 (after Task 5 merges)

**Key shared-file note:** Tasks 1 and 4 both modify `packages/core/src/lib.rs`. The change is a single `pub mod` line each. Rebase Task 4's branch onto `main` after Task 1 merges, resolve the trivial conflict, then re-run `/code-review-multi diff`.
