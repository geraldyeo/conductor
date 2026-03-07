# ADR-0005: Workspace & Session Metadata

## Status
Accepted

## Context

The orchestrator needs two foundational data-layer capabilities: (1) isolated filesystems for parallel agent sessions, and (2) persistent session metadata that survives restarts. These are the storage substrate beneath the lifecycle engine (ADR-0001) — every session transition reads or writes metadata, and every session spawn creates or destroys a workspace.

Four prior ADRs constrain the design:

1. **ADR-0001 (Session Lifecycle Engine)** defines the poll loop with three phases: gather (read metadata + agent info), evaluate (pure graph walk), transition (write metadata, execute entry actions including `destroyWorkspace()`). Metadata must support atomic writes for crash safety during transitions.
2. **ADR-0002 (Implementation Language)** locks in Rust, `tokio`, `async-trait`, and `CommandRunner` for subprocess management. All git and hook commands go through `CommandRunner`.
3. **ADR-0003 (Configuration System)** defines `ProjectConfig.symlinks` (repo-relative paths), `ProjectConfig.hooks` (4 lifecycle hooks as shell commands), and hash-based data directories at `~/.agent-orchestrator/{sha256-12chars}-{projectId}/`. Environment variables `AO_SESSION` and `AO_DATA_DIR` are set per-session by the engine.
4. **ADR-0004 (Plugin System)** defines `AgentWorkspaceHook` (agent-specific hooks merged with config hooks at creation time), `AgentSessionInfo` (branch, pr_url, tokens_in, tokens_out extracted by agents), session ID format `{prefix}-{issueId}-{attempt}`, and the static factory pattern for plugin registration.

Key forces:

- The PRD specifies git worktrees as the primary workspace strategy with clone as a fallback (FR2). Worktrees are fast and disk-efficient but share the `.git` object store.
- Session metadata must be human-readable, bash-scriptable, and require zero external dependencies (FR15). Flat `KEY=VALUE` files satisfy all three.
- The action journal (FR4/FR15) is append-only with structured entries for idempotency and retry decisions. This has fundamentally different access patterns from the read-modify-write metadata.
- Symlink escape prevention is a security requirement — workspace paths must be validated against the repo boundary.
- Multiple orchestrator instances with different configs must coexist without collision (hash-based directory namespacing with `.origin` collision detection).

## Considered Options

### Workspace-Metadata Coupling

1. **Fully independent modules** — Workspace trait (plugin slot) handles only filesystem isolation. SessionStore (core module, not a trait) handles only metadata persistence. The lifecycle engine coordinates between them. A `DataPaths` helper computes all paths for both modules.

2. **Workspace owns metadata** — The workspace plugin creates and manages session metadata alongside the worktree. `Workspace::create()` returns a handle that includes metadata access. Simpler engine code but couples persistence to the workspace plugin, forcing the `clone` plugin to duplicate metadata logic.

3. **Shared DataDir abstraction** — A `DataDir` object manages the entire `{hash}/` directory tree. Both workspace and metadata are facets of it. Clean path management but creates a god object that everything depends on.

### Action Journal Storage

4. **Separate JSONL file** — `sessions/{id}/journal.jsonl`, one JSON object per line. Append-only writes. Metadata file stays clean `KEY=VALUE`.

5. **Journal entries embedded in metadata** — Indexed keys like `JOURNAL_0=...` in the metadata file. One file per session but awkward format and the file grows with every action.

6. **Journal in a shared SQLite database** — Richer query support for idempotency checks. But adds a binary dependency and violates the zero-dependency principle.

### Directory Layout

7. **Separate top-level directories** — `{hash}/sessions/` for metadata, `{hash}/worktrees/` for git worktrees, `{hash}/archive/` for archived sessions. Each directory has distinct ownership and lifecycle.

8. **Unified per-session directory** — Everything under `{hash}/sessions/{id}/` including the worktree. Simpler mental model but deeply nested worktree paths.

9. **Flat sessions directory** — Metadata in `{hash}/sessions/`, worktrees elsewhere. Similar to option 7 but without the `archive/` directory.

## Decision

**Workspace-Metadata coupling:** Option 1 — fully independent modules. The Workspace trait is a plugin slot managing filesystem isolation. The SessionStore is a non-pluggable core module managing metadata. The lifecycle engine coordinates both. A `DataPaths` helper provides path computation for both modules.

**Action journal storage:** Option 4 — separate JSONL file per session. The metadata file stays clean `KEY=VALUE` for bash-scriptability. The journal uses JSONL for structured entries with fields like `action`, `target`, `dedupe_key`, `result`, `actor`. Different access patterns (atomic replace vs. append-only) get different file formats.

**Directory layout:** Option 7 — separate top-level directories matching ownership boundaries.

**The design has five components:**

### 1. DataPaths Helper

A lightweight struct computing all filesystem paths from the config hash. Not a trait, not pluggable — pure path arithmetic.

```rust
pub struct DataPaths {
    root: PathBuf,  // ~/.agent-orchestrator/{sha256-12chars}-{projectId}/
}

impl DataPaths {
    pub fn new(config_path: &Path, project_id: &str) -> Self;
    pub fn root(&self) -> &Path;
    pub fn origin_file(&self) -> PathBuf;              // {root}/.origin
    pub fn sessions_dir(&self) -> PathBuf;             // {root}/sessions/
    pub fn session_dir(&self, id: &str) -> PathBuf;    // {root}/sessions/{id}/
    pub fn metadata_file(&self, id: &str) -> PathBuf;  // {root}/sessions/{id}/metadata
    pub fn journal_file(&self, id: &str) -> PathBuf;   // {root}/sessions/{id}/journal.jsonl
    pub fn worktrees_dir(&self) -> PathBuf;            // {root}/worktrees/
    pub fn worktree_path(&self, id: &str) -> PathBuf;  // {root}/worktrees/{id}/
    pub fn archive_dir(&self) -> PathBuf;              // {root}/archive/
    pub fn archive_session_dir(&self, id: &str, timestamp: &str) -> PathBuf;
    pub fn ensure_dirs(&self) -> Result<(), DataPathsError>;
}
```

Directory layout:

```
~/.agent-orchestrator/{sha256-12chars}-{projectId}/
├── .origin                        # original config path (collision detection)
├── sessions/{id}/metadata         # KEY=VALUE flat file
├── sessions/{id}/journal.jsonl    # append-only action log
├── worktrees/{id}/                # git worktree checkout
└── archive/{id}-{YYYYMMDD}T{HHMMSS}Z/  # archived metadata + journal on delete (UTC)
```

The hash is `sha256(canonical_config_path)[..12]` — 48 bits of entropy, ~23.7 million distinct paths before 1% collision probability. The `.origin` file stores the canonical config path and provides a runtime safety net: `ensure_dirs()` checks `.origin` on startup. If a different config path produced the same hash, the orchestrator errors. If the `sessions/` directory is non-empty but `.origin` is missing (manual deletion), `ensure_dirs()` refuses to proceed rather than silently adopting another project's state.

### 2. Workspace Trait

A plugin slot following ADR-0004's static factory pattern. The trait manages only filesystem isolation — no hooks, no metadata. Implementations store their own path configuration (received via the factory function); the trait signature uses only `session_id` for lookup/destroy operations, and implementations derive paths internally.

```rust
pub struct WorkspaceCreateContext {
    pub session_id: String,
    pub repo_path: PathBuf,
    pub branch: String,
    pub base_branch: String,
    pub worktree_path: PathBuf,
    pub symlinks: Vec<PathBuf>,    // repo-relative paths
}

pub struct WorkspaceInfo {
    pub session_id: String,
    pub path: PathBuf,
    pub branch: String,
    pub base_branch: String,
}

#[async_trait]
pub trait Workspace: Send + Sync {
    fn meta(&self) -> PluginMeta;
    async fn create(&self, ctx: &WorkspaceCreateContext) -> Result<WorkspaceInfo, WorkspaceError>;
    async fn destroy(&self, session_id: &str) -> Result<(), WorkspaceError>;
    async fn exists(&self, session_id: &str) -> Result<bool, WorkspaceError>;
    async fn info(&self, session_id: &str) -> Result<Option<WorkspaceInfo>, WorkspaceError>;
    async fn list(&self) -> Result<Vec<WorkspaceInfo>, WorkspaceError>;
}
```

The `worktree` implementation: `create()` runs `git worktree add -b {branch} {worktree_path} {base_branch}`, creates validated symlinks, and returns `WorkspaceInfo`. `destroy()` runs `git worktree remove --force {worktree_path}`, then deletes the branch conditionally: `git branch -d` (lowercase, refuses to delete unmerged branches) for most terminal states; `git branch -D` (force) only when the session's termination reason indicates work is definitively abandoned (e.g., `manual_kill`). If `-d` fails due to unmerged work, the branch is preserved and a warning is logged. Both operations are idempotent.

**`destroy()` branch deletion strategy.** `destroy()` takes only `session_id`. The worktree implementation discovers the branch name by running `git worktree list --porcelain` and matching the worktree path (derived from `DataPaths` stored at construction). This avoids depending on naming conventions or reading from `SessionStore`.

**Symlink escape prevention:** before creating each symlink, both the target path and repo root are resolved via `std::fs::canonicalize()` and the target is verified to be within the repo root. Symlinks are repo-relative — `symlinks: [".env"]` creates `{worktree}/.env → {repo}/.env`. The TOCTOU window between validation and symlink creation is documented but accepted — the attacker would need write access to the repo directory during the brief window, and the orchestrator runs locally (not as a privileged service).

```rust
fn validate_symlink(target_path: &Path, repo_root: &Path) -> Result<(), WorkspaceError> {
    let resolved_target = std::fs::canonicalize(target_path)?;
    let resolved_repo = std::fs::canonicalize(repo_root)?;
    if !resolved_target.starts_with(&resolved_repo) {
        return Err(WorkspaceError::SymlinkEscape { .. });
    }
    Ok(())
}
```

Factory registration:

```rust
pub fn create_workspace(name: &str, config: &Config, paths: &DataPaths)
    -> Result<Box<dyn Workspace>, PluginError>
{
    match name {
        "worktree" => Ok(Box::new(WorktreeWorkspace::new(config, paths))),
        "clone" => Err(PluginError::NotImplemented("clone".into())),
        _ => Err(PluginError::UnknownPlugin(name.into())),
    }
}
```

### 3. SessionStore Module

A non-pluggable core module managing session metadata and action journals. Not a trait — the PRD specifies flat files as the only persistence backend.

**Metadata format** — `KEY=VALUE`, one per line, fixed field set. All values are single-line: the serializer replaces newlines with `\n` literal (two characters `\` and `n`) and backslashes with `\\`; the deserializer reverses this. Values containing `=` are handled correctly (split on first `=` only). The format is human-readable and debuggable with `cat`; however, the file should not be `source`-d in bash with untrusted values due to potential shell injection — use the orchestrator's `read()` API or simple parsing instead:

```bash
SESSION_ID=myproject-42-1
STATUS=working
CREATED_AT=2026-03-07T10:00:00Z
UPDATED_AT=2026-03-07T10:05:00Z
WORKSPACE_PATH=/path/to/worktree
AGENT=claude-code
RUNTIME=tmux
ISSUE_ID=42
ATTEMPT=1
BRANCH=
PR_URL=
TOKENS_IN=0
TOKENS_OUT=0
TERMINATION_REASON=
BASE_BRANCH=main
KILL_REQUESTED=false
TRACKER_CLEANUP_REQUESTED=false
```

The `KILL_REQUESTED` field provides durable kill intent — `ao session kill` writes this to disk before the next poll tick processes it, ensuring kill requests survive orchestrator restarts. The lifecycle engine's gatherer reads this field and populates `PollContext.manualKill`.

The `TRACKER_CLEANUP_REQUESTED` field serves the same purpose for `ao session cleanup` — a durable flag written to disk so cleanup requests are not lost on orchestrator restart. The lifecycle engine's gatherer reads this field and treats it equivalently to a kill triggered by tracker-terminal state detection.

`Option<TerminationReason>` serialization: `None` → empty value (`TERMINATION_REASON=`); `Some(variant)` → snake_case (e.g., `TERMINATION_REASON=budget_exceeded`).

**Journal format** — JSONL, one entry per line:

```json
{"action":"merge","target":"PR#123","timestamp":"2026-03-07T10:30:00Z","dedupe_key":"merge:PR#123:1709805000","result":"success","error_code":null,"attempt":1,"actor":"orchestrator"}
```

**Data types:**

```rust
/// SessionStatus is a shared enum in packages/core/src/types/status.rs,
/// imported by both the SessionStore and the lifecycle engine.
/// This avoids circular dependencies while providing compile-time type safety
/// on the system's most critical path (status is read/written every poll tick).
#[derive(Debug, Clone, PartialEq, Eq, Display, FromStr)]
#[strum(serialize_all = "snake_case")]
pub enum SessionStatus {
    Spawning, Working, PrOpen, ReviewPending, Approved, Mergeable,
    CiFailed, ChangesRequested, NeedsInput, Stuck,
    Killed, Terminated, Done, Cleanup, Errored, Merged,
}

/// TerminationReason is a constrained enum, not a free-form string.
/// Wrapped in Option<TerminationReason> in SessionMetadata — None means
/// "not terminated." Serializes to empty string in KEY=VALUE format.
#[derive(Debug, Clone, PartialEq, Eq, Display, FromStr)]
#[strum(serialize_all = "snake_case")]
pub enum TerminationReason {
    BudgetExceeded,
    ManualKill,
    StallTimeout,
    TrackerTerminal,
    AgentExit,
    SpawnFailed,
    MaxRetriesExceeded,
}

/// Session metadata. Uses custom KEY=VALUE serialization (not serde).
/// Values are single-line with newline escaping (\n → literal \n).
pub struct SessionMetadata {
    pub session_id: String,
    pub status: SessionStatus,
    pub created_at: String,
    pub updated_at: String,
    pub workspace_path: PathBuf,
    pub agent: String,
    pub runtime: String,
    pub issue_id: String,
    pub attempt: u32,
    pub branch: String,
    pub base_branch: String,
    pub pr_url: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub termination_reason: Option<TerminationReason>,  // None = not terminated, serializes to ""
    pub kill_requested: bool,                             // durable kill intent, survives restarts
    pub tracker_cleanup_requested: bool,                  // durable cleanup intent, survives restarts
}

#[derive(Serialize, Deserialize)]
pub struct JournalEntry {
    pub action: String,
    pub target: String,
    pub timestamp: String,
    pub dedupe_key: String,
    pub result: JournalResult,
    pub error_code: Option<String>,
    pub attempt: u32,
    pub actor: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalResult { Success, Failed, Skipped }
```

**API:**

```rust
pub struct SessionStore { paths: DataPaths }

impl SessionStore {
    pub fn new(paths: DataPaths) -> Self;
    pub fn create(&self, initial: &SessionMetadata) -> Result<(), StoreError>;
    pub fn read(&self, session_id: &str) -> Result<SessionMetadata, StoreError>;
    pub fn write(&self, session_id: &str, metadata: &SessionMetadata) -> Result<(), StoreError>;
    pub fn append_journal(&self, session_id: &str, entry: &JournalEntry) -> Result<(), StoreError>;
    pub fn read_journal(&self, session_id: &str) -> Result<Vec<JournalEntry>, StoreError>;
    pub fn archive(&self, session_id: &str) -> Result<(), StoreError>;
    pub fn list(&self) -> Result<Vec<SessionMetadata>, StoreError>;
    pub fn exists(&self, session_id: &str) -> Result<bool, StoreError>;
}
```

**Atomic write:** write to `{path}.tmp`, `fsync`, then `rename`. Atomic on POSIX. Note: full power-loss durability would require an additional `fsync` on the parent directory; this is deferred to post-MVP since process crashes (SIGKILL, panic) are the realistic failure mode on developer machines.

**Journal append durability:** `append_journal()` opens the file in append mode (`O_APPEND`), writes one JSONL line, and calls `fsync` before returning. This ensures journal entries — the idempotency mechanism for FR4 reactions — survive process crashes. `read_journal()` skips malformed trailing lines (with a warning log) rather than failing the entire read, providing resilience against partial writes from crashes mid-append.

**Race-free creation:** `std::fs::create_dir()` — atomic on POSIX. Two concurrent creators get one success and one `AlreadyExists` error.

**Session ID validation:** reject empty, `/`, `\`, `\0`, `.`, `..`, and leading-dot IDs. Defense in depth alongside ADR-0004's tmux-safe format `[a-zA-Z0-9._-]`.

**Archive on delete:** rename `sessions/{id}/` → `archive/{id}-{YYYYMMDD}T{HHMMSS}Z/` (UTC, ISO 8601 basic format). Timestamped to prevent collisions on reuse.

### 4. Hook Execution

The lifecycle engine orchestrates hooks — the workspace trait does not execute them. This keeps hook logic centralized and prevents the `clone` plugin from duplicating it.

```rust
pub struct HookContext {
    pub session_id: String,
    pub workspace_path: PathBuf,
    pub repo_path: PathBuf,
    pub data_dir: PathBuf,
}

/// Default hook timeout: 60 seconds.
/// Per-hook timeout configuration is a post-MVP config extension in ADR-0003.
pub const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn run_hook(command: &str, ctx: &HookContext, timeout: Duration) -> Result<(), HookError>;
```

Hooks run via `CommandRunner` with `cwd = workspace_path`. Environment injected: `AO_SESSION`, `AO_DATA_DIR`, `AO_WORKSPACE_PATH`, `AO_REPO_PATH`. Timeout enforced via SIGTERM then SIGKILL.

**Hook ordering:**

| Event | Hooks (in order) |
|-------|-----------------|
| Workspace created | config `afterCreate` → agent `AfterCreate` hooks |
| Before each attempt | config `beforeRun` |
| After each attempt | config `afterRun` |
| Before destruction | config `beforeRemove` |

Agent hooks run after config hooks per ADR-0004. Note: `AgentWorkspaceHook::PostToolUse` (ADR-0004 line 221) is installed into the workspace at creation time as a file (e.g., a Claude Code hook script), not executed as a lifecycle hook. It runs during agent execution, triggered by the agent runtime, not at a lifecycle boundary.

**`beforeRemove` failure semantics:** `beforeRemove` hook failures are logged but do not prevent workspace destruction or archival. Destruction must be best-effort to avoid leaked resources.

### 5. Lifecycle Engine Integration

**Session creation sequence:**

1. Compute `session_id` from `{prefix}-{issueId}-{attempt}`
2. `session_store.create(initial_metadata)` — race-free reservation
3. `workspace.create(WorkspaceCreateContext)` — git worktree + symlinks
4. `session_store.write()` — record `WORKSPACE_PATH`, `BASE_BRANCH`
5. Run config `afterCreate` hook
6. Run agent `AfterCreate` hooks
7. Install agent `PostToolUse` hooks (write script files into workspace)
8. Run config `beforeRun` hook
9. Set `AO_SESSION` and `AO_DATA_DIR` environment variables
10. Execute `LaunchPlan` via Runtime (ADR-0004)

**Unwind on failure (steps 2-8):** if any step in the pre-launch sequence (steps 2 through 8) fails, the engine unwinds all prior steps: destroy workspace via `DataPaths::worktree_path(session_id)` (not from metadata, since `WORKSPACE_PATH` may not have been written yet), archive metadata with `TERMINATION_REASON=SpawnFailed`. Step 9 (env setup) cannot fail. Step 10 (plan execution) failures are handled by ADR-0004's plan failure semantics (transition to `errored`, destroy runtime).

**Session destruction sequence** (from terminal-state entry actions):

1. Run config `beforeRemove` hook (failure logged, does not block destruction)
2. `workspace.destroy(session_id)` — worktree remove + conditional branch deletion
3. `session_store.archive(session_id)` — move to archive/

Called from lifecycle engine entry actions for terminal states (`cleanup`, `killed`, etc.). ADR-0001 lists `destroyWorkspace()` as an entry action; archival is an additional step owned by this ADR.

**Metadata updates during poll loop** (per ADR-0001 phases):

- **Gather:** `session_store.read()` + `Agent::parse_session_info()` for fresh `AgentSessionInfo`
- **Evaluate:** pure graph walk, no I/O
- **Transition:** `session_store.write()` with updated fields; `session_store.append_journal()` for entry actions

### Module Structure

```
packages/core/src/
├── types/
│   └── status.rs           # SessionStatus, TerminationReason (shared enums)
├── workspace/
│   ├── mod.rs              # Workspace trait, types, factory
│   ├── worktree.rs         # WorktreeWorkspace implementation
│   └── error.rs            # WorkspaceError
├── store/
│   ├── mod.rs              # SessionStore, DataPaths
│   ├── metadata.rs         # SessionMetadata, KEY=VALUE serialization
│   ├── journal.rs          # JournalEntry, JSONL serialization
│   └── error.rs            # StoreError
└── hooks.rs                # HookContext, run_hook(), DEFAULT_HOOK_TIMEOUT
```

Reference `docs/plans/2026-03-07-workspace-session-metadata-design.md` for full pseudocode, testing strategy, and deferred items.

## Consequences

Positive:

- Workspace and metadata are fully decoupled — each module is independently testable. The workspace trait can be tested with a real git repo in a temp dir; the SessionStore can be tested with pure filesystem operations. Neither depends on the other.
- The `DataPaths` helper centralizes all path computation without becoming a god object — it computes paths but owns no state and performs no I/O (except `ensure_dirs()`).
- Flat `KEY=VALUE` metadata files are human-readable and debuggable with `cat`. Newline escaping keeps values single-line without sacrificing content. No external dependencies.
- `SessionStatus` and `TerminationReason` as shared enums in `types/status.rs` provide compile-time type safety on the system's hottest path (status read/written every poll tick) without circular dependencies between the store and lifecycle engine.
- JSONL journals cleanly separate the append-only access pattern from atomic-replace metadata. Structured entries with `dedupe_key` support FR4 idempotency checks. Journal appends are fsynced for durability; malformed trailing lines from crash-interrupted writes are skipped with a warning.
- Atomic writes (fsync + rename) and race-free creation (`create_dir` atomicity) provide crash safety and concurrency safety without locks or transactions.
- Symlink escape prevention via `canonicalize()` + prefix check is defense in depth on top of session ID validation. The TOCTOU window is documented and accepted for a local developer tool.
- The lifecycle engine's hook orchestration follows the same pattern as ADR-0004's plan execution — the engine coordinates between plugins, plugins stay narrowly scoped.
- Archive-on-delete provides an audit trail without additional infrastructure. UTC timestamps in ISO 8601 basic format prevent timezone ambiguity.
- Conditional branch deletion (`git branch -d` by default, `-D` only for abandoned work) preserves unmerged work for recovery in error states.
- The unwind boundary covers all pre-launch steps (2-8), using `DataPaths` directly for workspace location (independent of metadata state).

Negative:

- The lifecycle engine has more orchestration code — it coordinates workspace creation, metadata creation, hook execution, and error unwinding. This is deliberate (engine-as-coordinator pattern from ADR-0004) but adds complexity to the spawn sequence.
- No cross-session atomic updates — archiving two sessions simultaneously is not atomic relative to each other. Acceptable for MVP data volumes; the PRD acknowledges this limitation.
- Flat-file listing requires scanning the `sessions/` directory and reading each metadata file. Performance degrades at high session counts. Unlikely to be practical at MVP scale; SQLite migration is a documented upgrade path.
- The `worktree` implementation depends on `git worktree` CLI behavior (output format of `--porcelain`, `--force` flag semantics). Git version changes could break parsing. Mitigated by testing against a real git repo in CI.
- Archive directories accumulate without automatic cleanup. A garbage collection sweep is deferred to post-MVP.
- Hook timeout enforcement relies on process signal delivery (SIGTERM/SIGKILL), which may behave differently across platforms. macOS and Linux are the primary targets.
- Full power-loss durability for metadata would require an additional `fsync` on the parent directory after rename. Deferred — process crash safety (the realistic failure mode) is covered by the current approach.
- `Workspace::destroy()` discovering the branch name via `git worktree list --porcelain` adds a subprocess call. Acceptable overhead for an infrequent operation. If the worktree was already removed (manual deletion), the branch cannot be discovered and becomes orphaned — documented as a known limitation with manual cleanup via `git branch --list`.
- Concurrent journal appends from multiple writers (e.g., reaction engine in FR4) will require serialization (per-session mutex or channel). At MVP, the lifecycle engine is the sole writer per session during its poll tick, so this is not a concern. Deferred to FR4 implementation.
