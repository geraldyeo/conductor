# Configuration System Design

**Date:** 2026-03-06
**FR:** FR10 (Configuration System)
**ADR:** 0003
**Status:** Draft
**Depends on:** ADR-0002 (Implementation Language — Rust, serde, garde)

## Overview

The configuration system is the foundational layer that every other subsystem reads from: the lifecycle engine (ADR-0001), CLI (FR6), agent plugins (FR1), tracker plugins (FR16), and the prompt system (FR11). It loads, validates, and provides typed access to `agent-orchestrator.yaml`.

## Approach

**Monolithic Typed Struct** — a single `Config` struct hierarchy with `serde` deserialization and `garde` validation. All fields are known at compile time. Post-MVP fields are `Option<T>` and ignored until their FRs land.

### Alternatives Considered

1. **Layered Config with Explicit Merge** — separate partial-config structs for home dir, project dir, env vars, and CLI flags, merged bottom-up. Rejected: adds two struct variants per config level and merge boilerplate for every field, with little MVP value. Multi-file layering can be added later without schema changes.

2. **Schema-Driven with Plugin Extension Points** — core config handles orchestrator fields only; plugins register additional config schemas at runtime via a trait method returning JSON Schema. Rejected: loses compile-time type safety, requires a JSON Schema validator dependency, and is over-engineered when we have ~3 plugins at MVP.

## 1. Config Schema

### Top-Level Config

```rust
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    // --- MVP fields ---
    #[serde(default = "default_port")]
    pub port: u16,                              // 3000

    #[serde(default)]
    pub defaults: Defaults,

    #[garde(length(min = 1))]
    pub projects: HashMap<String, ProjectConfig>, // required, at least 1

    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_agents: u16,             // 10

    // --- Post-MVP (present but not validated yet) ---
    #[serde(default)]
    pub terminal_port: Option<u16>,
    #[serde(default)]
    pub direct_terminal_port: Option<u16>,
    #[serde(default)]
    pub ready_threshold_ms: Option<u64>,
    #[serde(default)]
    pub max_retry_backoff_ms: Option<u64>,
    #[serde(default)]
    pub max_session_tokens: Option<u64>,
    #[serde(default)]
    pub max_session_wall_clock_ms: Option<u64>,
    #[serde(default)]
    pub max_retries_per_issue: Option<u16>,
    #[serde(default)]
    pub notifiers: Option<HashMap<String, Value>>,
    #[serde(default)]
    pub notification_routing: Option<Value>,
    #[serde(default)]
    pub reactions: Option<Value>,
}
```

### Defaults

```rust
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    #[serde(default = "default_runtime")]   // "tmux"
    pub runtime: String,
    #[serde(default = "default_agent")]     // "claude-code"
    pub agent: String,
    #[serde(default = "default_workspace")] // "worktree"
    pub workspace: String,
    #[serde(default)]
    pub notifiers: Vec<String>,
}
```

### Per-Project Config

```rust
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub name: Option<String>,

    #[garde(pattern(r"^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+$"))]
    pub repo: String,                          // required: "owner/repo"

    #[garde(custom(validate_path_exists))]
    pub path: String,                          // required: local path (supports ~)

    #[serde(default = "default_branch")]       // "main"
    pub default_branch: String,

    #[serde(default)]
    pub session_prefix: Option<String>,

    // Plugin overrides (None = inherit from defaults)
    pub runtime: Option<String>,
    pub agent: Option<String>,
    pub workspace: Option<String>,

    #[serde(default)]
    pub tracker: TrackerConfig,

    pub scm: Option<ScmConfig>,

    #[serde(default)]
    pub symlinks: Vec<String>,

    #[serde(default)]
    pub hooks: Hooks,

    #[serde(default)]
    pub agent_config: Option<AgentConfig>,

    pub agent_rules: Option<String>,
    pub agent_rules_file: Option<String>,
    pub orchestrator_rules: Option<String>,

    // Post-MVP
    pub reactions: Option<Value>,
}
```

### Supporting Structs

```rust
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackerConfig {
    #[serde(default = "default_tracker")]   // "github"
    pub plugin: String,
    pub team_id: Option<String>,
    #[serde(default)]
    pub active_states: Vec<String>,
    #[serde(default)]
    pub terminal_states: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScmConfig {
    #[serde(default = "default_scm")]       // "github"
    pub plugin: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Hooks {
    pub after_create: Option<String>,
    pub before_run: Option<String>,
    pub after_run: Option<String>,
    pub before_remove: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    #[serde(default = "default_max_turns")] // 20
    pub max_turns: u16,
    #[serde(default)]
    pub permissions: Option<String>,        // "skip" | "default"
    pub model: Option<String>,
    #[serde(default = "default_sandbox")]   // "workspace-write"
    pub sandbox: String,

    /// Forward-compat for agent-specific fields.
    /// When FR1 (Agent Plugin) lands, each agent validates its own extras.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
```

### Design Decisions

- **`camelCase` in YAML** matches the PRD's field names (JavaScript/YAML convention). Rust fields use `snake_case` with `#[serde(rename_all = "camelCase")]`.
- **Post-MVP fields are `Option<Value>`** — they deserialize without error but aren't validated until their FRs land. This prevents config files from breaking when users add fields early.
- **`AgentConfig.extra` via `#[serde(flatten)]`** — forward-compatibility seam for agent-specific config. Core validates known fields; agents validate their own extras when FR1 lands.
- **`projects` is `HashMap<String, ProjectConfig>`** — the key is the project ID, used throughout the system (session naming, data directory, CLI commands).

## 2. Config Discovery and Loading

### Discovery Order

Follows PRD Section 4 (FR10):

1. `AO_CONFIG_PATH` environment variable — if set, use directly (error if file doesn't exist)
2. Walk up directory tree from CWD — check each ancestor for `agent-orchestrator.yaml` / `.yml`
3. Home directory fallback: `~/.agent-orchestrator.yaml`, `~/.agent-orchestrator.yml`, `~/.config/agent-orchestrator/config.yaml`

```rust
pub fn discover_config_path() -> Result<PathBuf, ConfigError> {
    // 1. Env var override
    if let Ok(path) = std::env::var("AO_CONFIG_PATH") {
        let p = PathBuf::from(path);
        if p.exists() { return Ok(p); }
        return Err(ConfigError::EnvPathNotFound(p));
    }

    // 2. Walk up from CWD
    let cwd = std::env::current_dir()?;
    let mut dir = cwd.as_path();
    loop {
        for name in ["agent-orchestrator.yaml", "agent-orchestrator.yml"] {
            let candidate = dir.join(name);
            if candidate.exists() { return Ok(candidate); }
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
        if path.exists() { return Ok(path); }
    }

    Err(ConfigError::NotFound)
}
```

### Loading Pipeline

```
discover_config_path()
  -> read file to string
  -> serde_yml::from_str::<Config>()       // Pass 1: structural
  -> config.resolve_defaults()             // inherit top-level defaults into projects
  -> config.validate(&())                  // Pass 2: semantic (garde)
  -> Ok(Config)
```

### Default Resolution

Post-deserialization pass fills `None` project fields from top-level defaults:

```rust
impl Config {
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
```

### Secrets

Environment-variable secrets live in a separate struct, never serialized to disk:

```rust
pub struct ResolvedSecrets {
    pub linear_api_key: Option<String>,    // LINEAR_API_KEY
    pub slack_webhook_url: Option<String>,  // SLACK_WEBHOOK_URL
    pub composio_api_key: Option<String>,   // COMPOSIO_API_KEY
}
```

Secrets are excluded from `Config` to prevent accidental logging, serialization, or exposure in error messages. The `ResolvedSecrets` struct has no `Serialize` derive.

## 3. Validation and Error Reporting

### Two-Pass Validation

**Pass 1 — Structural (serde deserialization):** catches missing required fields, wrong types, malformed YAML.

**Pass 2 — Semantic (garde + custom validators):** catches cross-field constraints after deserialization succeeds.

| Rule | Mechanism |
|------|-----------|
| `projects` non-empty | `garde(length(min = 1))` |
| `repo` format `"owner/repo"` | `garde(pattern(...))` |
| `path` exists on disk | Custom: `validate_path_exists` |
| `permissions` value | Custom: must be `"skip"` or `"default"` if set |
| `sandbox` value | Custom: must be `"workspace-write"`, `"read-only"`, or `"full"` |
| Plugin names known | Custom: runtime in `{"tmux", "process"}`, agent in `{"claude-code", "codex", ...}` |
| `agent_rules` / `agent_rules_file` mutual exclusion | Custom: at most one may be set per project |

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config not found (searched CWD ancestors and ~/.config/)")]
    NotFound,

    #[error("AO_CONFIG_PATH={0} does not exist")]
    EnvPathNotFound(PathBuf),

    #[error("could not determine home directory")]
    NoHomeDir,

    #[error("{path}: {source}")]
    Parse {
        path: PathBuf,
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

### Terminal Output

Errors include the config file path, the field path, and a human-readable message:

```
agent-orchestrator.yaml: validation failed:
  projects.myapp.repo: must match "owner/repo" pattern, got "just-a-name"
  projects.myapp.path: path does not exist: ~/Code/nonexistent
  projects.myapp.agentConfig.permissions: expected "skip" or "default", got "yolo"
```

### Deferred

- Warning-level validation (e.g., "path exists but is not a git repo") — post-MVP
- Config linting command (`ao config check`) — belongs to FR6
- Validation of post-MVP `Option<Value>` fields — validated when their FRs land

## 4. Public API and Module Structure

### Public API

```rust
// src/config/mod.rs

/// Load and validate config from disk. Primary entry point.
pub fn load() -> Result<Config, ConfigError>;

/// Load from a specific path (for `ao init --check` and tests).
pub fn load_from_path(path: &Path) -> Result<Config, ConfigError>;

/// Resolve secrets from environment variables.
pub fn load_secrets() -> ResolvedSecrets;

/// Generate a default config YAML string (for `ao init`).
pub fn generate_default(project_id: &str, repo: &str, path: &str) -> String;
```

### Module Layout

```
packages/core/src/
├── config/
│   ├── mod.rs          # Public API: load(), load_from_path(), generate_default()
│   ├── schema.rs       # Config, ProjectConfig, Defaults, AgentConfig, etc.
│   ├── discovery.rs    # discover_config_path()
│   ├── validation.rs   # Custom validators (path_exists, plugin_name, etc.)
│   ├── secrets.rs      # ResolvedSecrets from env vars
│   └── error.rs        # ConfigError enum
└── lib.rs              # pub mod config;
```

### Ownership Model

`Config` is loaded once at startup, then shared via `&Config` or `Arc<Config>`. No interior mutability needed for MVP since there's no hot-reload.

**Upgrade path to hot-reload:** when hot-reload lands post-MVP, the internal storage changes to `ArcSwap<Config>`. Readers get a cheap clone of the Arc. The public API stays the same.

### Consumer Examples

```rust
// Lifecycle engine (ADR-0001)
let config = ao_core::config::load()?;
let engine = LifecycleEngine::new(config);

// CLI: ao init
let yaml = ao_core::config::generate_default("myapp", "owner/myapp", "~/Code/myapp");
std::fs::write("agent-orchestrator.yaml", yaml)?;

// CLI: ao start
let config = ao_core::config::load()?;
let secrets = ao_core::config::load_secrets();

// Plugin factory
let project = &config.projects["myapp"];
let agent_name = project.agent.as_deref().unwrap(); // resolved from defaults
```

### Example Config File

What `ao init` generates:

```yaml
port: 3000
maxConcurrentAgents: 10

defaults:
  runtime: tmux
  agent: claude-code
  workspace: worktree

projects:
  myapp:
    repo: owner/myapp
    path: ~/Code/myapp
    defaultBranch: main
    tracker:
      plugin: github
```

## Deferred to Post-MVP

| Feature | Deferred To | Reason |
|---------|-------------|--------|
| Hot-reload (file watcher, last-known-good) | Post-MVP | MVP can restart to pick up changes |
| Multi-file config layering (home + project) | Post-MVP | Single file sufficient for MVP |
| `startDir` CLI parameter | FR6 (CLI ADR) | CLI flag, not config system concern |
| `notifiers`, `notificationRouting` validation | FR12 (Notification Routing) | Stored as `Option<Value>` until then |
| `reactions` validation | FR4 (Reactions) | Stored as `Option<Value>` until then |
| `maxConcurrentAgentsByState` validation | FR5 (Scheduling) | Stored as `Option<Value>` until then |
| Config linting command (`ao config check`) | FR6 (CLI ADR) | CLI surface, not config internals |
| Warning-level diagnostics | Post-MVP | Only errors for now |

## Dependencies

- **Upstream:** ADR-0002 (Rust, serde, garde, thiserror)
- **Downstream consumers:**
  - ADR-0001 (Session Lifecycle Engine) — reads `maxConcurrentAgents`, project configs
  - FR1 (Agent Plugin) — reads `AgentConfig`, validates `extra` fields
  - FR6 (CLI) — calls `load()`, `generate_default()`
  - FR11 (Prompt System) — reads `agentRules`, `agentRulesFile`
  - FR16 (Tracker Integration) — reads `TrackerConfig`

## Crate Dependencies

| Crate | Purpose |
|-------|---------|
| `serde` + `serde_yml` | YAML deserialization with derive |
| `serde_json` | `Value` type for post-MVP fields and `AgentConfig.extra` |
| `garde` | Declarative validation with derive |
| `thiserror` | Error type derive |
| `dirs` | Home directory resolution |
| `shellexpand` | Tilde expansion in paths |
