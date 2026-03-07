# ADR-0003: Configuration System

## Status
Accepted

## Context

The configuration system is the foundational layer that every subsystem reads from: the lifecycle engine (ADR-0001), CLI (FR6), agent plugins (FR1), tracker plugins (FR16), and the prompt system (FR11). It must load, validate, and provide typed access to `agent-orchestrator.yaml`.

Three constraints shape the design:

1. **Every downstream FR depends on config.** CLI commands (`ao init`, `ao start`, `ao spawn`) read project definitions. Agent plugins read `agentConfig`. Tracker plugins read `tracker.plugin` and state mappings. The config schema is the contract between subsystems.
2. **The PRD specifies ~40 config options** across top-level and per-project scopes (FR10, Section 4). MVP needs a subset; the schema must accept but ignore post-MVP fields so early adopters' config files don't break as features land.
3. **ADR-0002 locked in the tech stack**: `serde` + `serde_yml` for deserialization, `garde` for validation, Rust structs for type safety. The question is how to structure the schema, discovery, validation, and public API — not which libraries to use.

The PRD requires hot-reload (watch file, reject invalid changes, keep last-known-good config). This is deferred to post-MVP; the system loads config once at startup.

## Considered Options

1. **Monolithic Typed Struct** — A single `Config` struct hierarchy with all fields known at compile time. `serde(default)` for optional fields, `garde::Validate` for semantic validation. Post-MVP fields stored as `Option<T>` or `Option<Value>`. Plugin-specific config uses `#[serde(flatten)]` for forward-compatibility.

2. **Layered Config with Explicit Merge** — Separate partial-config structs (`PartialConfig` with all fields `Option<T>`) for each source layer: built-in defaults, home directory config, project config, env var overrides, CLI flag overrides. A `merge()` function combines layers bottom-up into a resolved `Config`. Enables multi-file config splitting.

3. **Schema-Driven with Plugin Extension Points** — Core config struct handles orchestrator-owned fields. Each plugin slot registers additional config schemas at runtime via a trait method returning JSON Schema. Plugin-specific config lives in `serde_json::Value` blobs, validated by the plugin at load time.

## Decision

Option 1: Monolithic Typed Struct.

**Why not Option 2 (Layered Merge)?** It requires two struct variants per config level (partial + resolved) and merge logic for every field. Multi-file config splitting provides little MVP value — a single YAML file is sufficient. Layered merging can be added later without schema changes by introducing a `PartialConfig` variant that deserializes into the existing `Config` via a builder.

**Why not Option 3 (Schema-Driven)?** It loses compile-time type safety for plugin config, requires a JSON Schema validator dependency, and is over-engineered when we have ~3 plugins at MVP. ADR-0002 chose Rust specifically for exhaustive matching and explicit traits; dynamic schema validation works against that choice.

**Why Option 1?** It plays to Rust's strengths: the compiler enforces that every config access is type-checked. `serde(default)` handles optional fields cleanly. `#[serde(flatten)]` on `AgentConfig.extra` provides a forward-compatibility seam — core validates known fields, agent plugins validate their own extras when FR1 lands. Post-MVP fields use `Option<Value>` and deserialize without error, so users can add them to config files early without breakage.

**Key design decisions:**

### Schema

- **`camelCase` in YAML** matches PRD field names. Rust structs use `snake_case` with `#[serde(rename_all = "camelCase")]`.
- **`projects` is `HashMap<String, ProjectConfig>`** — the key is the project ID used throughout the system.
- **Per-project plugin fields (`runtime`, `agent`, `workspace`) are `Option<String>`** — `None` inherits from `defaults`.
- **`AgentConfig.extra: HashMap<String, Value>`** via `#[serde(flatten)]` absorbs agent-specific fields without schema changes.
- **Budget enforcement fields** (`maxSessionTokens`, `maxSessionWallClockMs`) are `Option<u64>` — typed at MVP to support ADR-0001's `budgetExceeded` global edge at precedence 1.
- **Post-MVP fields** (`notifiers`, `notificationRouting`, `reactions`, `maxConcurrentAgentsByState`) are `Option<Value>` — accepted but not validated until their FRs land.
- **No `deny_unknown_fields`** on `Config` or `ProjectConfig` — this allows forward-compatibility with fields from newer orchestrator versions. Nested structs with stable field sets (e.g., `Hooks`) may use `deny_unknown_fields` to catch typos.
- **`AO_SESSION` and `AO_DATA_DIR`** environment variables (PRD lines 256-257) are set per-session by the runtime/lifecycle engine, not read by the config loader. They are not part of this ADR's scope.

### Config Discovery

Follows PRD Section 4 (FR10) search order. The discovery function accepts an optional `start_dir` parameter (defaulting to CWD) so the CLI can pass `startDir` when FR6 lands without a breaking signature change:

```rust
pub fn discover_config_path(start_dir: Option<&Path>) -> Result<PathBuf, ConfigError>
```

1. `AO_CONFIG_PATH` environment variable (direct path, error if missing)
2. Walk up directory tree from `start_dir` (or CWD if `None`) for `agent-orchestrator.yaml` / `.yml`
3. Home directory fallback: `~/.agent-orchestrator.yaml`, `~/.agent-orchestrator.yml`, `~/.config/agent-orchestrator/config.yaml`, `~/.config/agent-orchestrator/config.yml`

### Loading Pipeline

```
discover_config_path()
  → read file to string
  → serde_yml::from_str::<Config>()       // Pass 1: structural
  → config.resolve_defaults()             // inherit defaults into projects
  → config.validate(&())                  // Pass 2: semantic (garde)
  → Ok(Config)
```

### Validation (Two-Pass)

**Pass 1 — Structural:** `serde` deserialization catches missing required fields, wrong types, malformed YAML.

**Pass 2 — Semantic:** `garde` + custom validators for cross-field constraints:
- `projects` non-empty (`garde(length(min = 1))`)
- `repo` matches `"owner/repo"` pattern (`garde(pattern(...))`)
- `path` exists on disk (custom validator: tilde expansion via `shellexpand`, then check expanded path; tilde expansion failure produces a validation error; symlinks are not resolved; relative paths are resolved against the config file's parent directory)
- `permissions` ∈ `{"skip", "default"}`, `sandbox` ∈ `{"workspace-write", "read-only", "full"}`
- Plugin names are known values (runtime, agent, workspace, tracker, scm) — see Consequences for extensibility note
- `agent_rules` and `agent_rules_file` are mutually exclusive per project
- `maxSessionTokens` and `maxSessionWallClockMs`, when present, must be positive integers (> 0) — enforced via `garde(range(min = 1))` or custom validator

### Secrets

Environment-variable secrets (`LINEAR_API_KEY`, `SLACK_WEBHOOK_URL`, `COMPOSIO_API_KEY`) live in a separate `ResolvedSecrets` struct with no `Serialize` derive, preventing accidental logging or disk writes.

### Public API

Four functions in `packages/core/src/config/mod.rs`:
- `load(start_dir: Option<&Path>) -> Result<Config, ConfigError>` — discover + load + validate
- `load_from_path(path: &Path) -> Result<Config, ConfigError>` — load from explicit path
- `load_secrets() -> ResolvedSecrets` — env var secrets
- `generate_default(project_id, repo, path) -> String` — default YAML for `ao init` (must round-trip through `serde_yml` deserialization in tests to prevent drift)

### Module Structure

```
packages/core/src/config/
├── mod.rs          # Public API
├── schema.rs       # Config, ProjectConfig, Defaults, etc.
├── discovery.rs    # discover_config_path()
├── validation.rs   # Custom validators
├── secrets.rs      # ResolvedSecrets
└── error.rs        # ConfigError
```

### Ownership Model

`Config` is loaded once, shared via `&Config` or `Arc<Config>`. No interior mutability at MVP. Upgrade path: `ArcSwap<Config>` for hot-reload post-MVP.

### Deferred to Post-MVP

| Feature | Deferred To | Reason |
|---------|-------------|--------|
| Hot-reload | Post-MVP | Restart to pick up changes |
| Multi-file layering | Post-MVP | Single file sufficient |
| `notifiers` / `reactions` validation | FR4 / FR12 | `Option<Value>` until then |
| `maxConcurrentAgentsByState` validation | FR5 | Present as `Option<Value>`, validated when FR5 lands |
| `startDir` CLI parameter | FR6 | `discover_config_path()` accepts optional `start_dir` now; CLI exposes it later |
| Config linting (`ao config check`) | FR6 | CLI surface |
| `AgentConfig.extra` validation | FR1 | Extras are unvalidated `HashMap<String, Value>` until agent plugins validate their own fields |
| `defaults.notifiers` default value | FR12 | PRD defaults to `["composio", "desktop"]`; MVP defaults to empty vec since notifiers are post-MVP |
| Duplicate YAML key detection | Post-MVP | `serde_yml` silently takes last value; config linter should warn |

## Consequences

**Positive:**
- Compile-time type safety for all config access — misspelled field names are caught at build time, not at runtime.
- Two-pass validation gives clear, actionable error messages with file path and field path.
- `serde(default)` + `Option<Value>` for post-MVP fields means config files are forward-compatible — users can add fields before their FRs land without breaking validation.
- `#[serde(flatten)]` on `AgentConfig.extra` lets agent plugins extend config without core schema changes.
- Small public API (4 functions) keeps the surface area minimal for downstream consumers.
- Walk-up-tree discovery follows established conventions (git, npm, cargo) — users don't need to learn a new pattern.

**Negative:**
- Adding new validated config fields requires recompiling — but that's expected and desirable in Rust (the compiler catches misuse of new fields).
- Plugin-specific config validation is deferred to FR1 — until then, `AgentConfig.extra` accepts any key/value without validation. Users may get confusing late errors if an agent plugin rejects an extra field at runtime rather than at config load time. `#[serde(flatten)]` combined with `rename_all` has known edge cases with `serde_yml` — implementation must include integration tests for flatten + rename_all behavior.
- No hot-reload at MVP means config changes require an orchestrator restart.
- Single-file config means users can't split global defaults from project-specific overrides into separate files.
- Plugin name validation is hard-coded in the config module. Adding a new plugin (e.g., a third-party runtime) requires modifying the valid-names list and recompiling core. Post-MVP, consider an `allowUnknownPlugins: true` config option or plugin discovery mechanism.
- Path validation runs at config load time against the local filesystem. A shared team config referencing paths that exist on some machines but not others will fail validation. Workaround: use per-machine config files or defer path validation to the CLI layer per-project.
- **`serde_yml` stability risk.** `serde_yml` is the maintained successor to the archived `serde_yaml`, but has fewer maintainers and a smaller user base. Fallback plan if `serde_yml` stalls: vendor the crate, or switch to a two-pass approach (parse YAML to `serde_json::Value` via `yaml-rust2`, then deserialize from Value via `serde_json::from_value`). Pin the version in `Cargo.toml`.
