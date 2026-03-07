# Workspace & Session Metadata Design

Reference design for ADR-0005. Covers FR2 (Isolated Parallel Workspaces) and FR15 (Session Metadata System).

## Dependencies

- **ADR-0001** (Session Lifecycle Engine): poll loop phases (gather/evaluate/transition), entry actions call workspace create/destroy, session metadata read/written during gather and transition.
- **ADR-0002** (Implementation Language): Rust, `tokio`, `async-trait`, `serde`, `strum` (for enum Display/FromStr), `CommandRunner` for subprocess management.
- **ADR-0003** (Configuration System): `ProjectConfig.symlinks`, `ProjectConfig.hooks` (Hooks struct), hash-based data dir `~/.agent-orchestrator/{hash}/`, `AO_SESSION` / `AO_DATA_DIR` env vars.
- **ADR-0004** (Plugin System): `AgentWorkspaceHook` enum (`PostToolUse`, `AfterCreate`), `Agent::workspace_hooks()`, session ID format `{prefix}-{issueId}-{attempt}`, `AgentSessionInfo` struct, static factory pattern.

## 1. DataPaths Helper

A lightweight, non-pluggable struct that computes all filesystem paths from the config hash. Used by both the Workspace trait and SessionStore module.

```rust
pub struct DataPaths {
    root: PathBuf,  // ~/.agent-orchestrator/{sha256-12chars}-{projectId}/
}

impl DataPaths {
    /// Compute root from config path and project ID.
    /// Hash = sha256(canonical_config_path)[..12] — 48 bits of entropy.
    /// Birthday-bound: ~23.7 million distinct paths before 1% collision probability.
    /// The .origin file provides a runtime safety net for the theoretical collision case.
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
        // {root}/archive/{id}-{YYYYMMDD}T{HHMMSS}Z/

    /// Create sessions/, worktrees/, archive/ if missing.
    /// Write .origin if absent and sessions/ is empty.
    /// Validate .origin matches if present.
    /// Refuse to proceed if sessions/ is non-empty but .origin is missing
    /// (prevents silent state adoption after manual .origin deletion).
    pub fn ensure_dirs(&self) -> Result<(), DataPathsError>;
}
```

### Directory Layout

```
~/.agent-orchestrator/{sha256-12chars}-{projectId}/
├── .origin                              # original config path (collision detection)
├── sessions/{id}/metadata               # KEY=VALUE flat file
├── sessions/{id}/journal.jsonl          # append-only action log
├── worktrees/{id}/                      # git worktree checkout
└── archive/{id}-{YYYYMMDD}T{HHMMSS}Z/  # archived metadata + journal on delete (UTC)
```

### Hash Collision Detection

The `.origin` file stores the canonical config path that produced this hash. On startup, `ensure_dirs()` reads `.origin` and compares it to the current config path. If they differ, the orchestrator returns an error explaining the collision rather than silently sharing state. If `sessions/` is non-empty but `.origin` is missing (e.g., manual deletion), `ensure_dirs()` also errors — this prevents a new project from silently inheriting another project's sessions.

## 2. Workspace Trait

A plugin slot following ADR-0004's static factory pattern. MVP implementation: `worktree`. Planned: `clone`.

### Context and Result Types

```rust
pub struct WorkspaceCreateContext {
    pub session_id: String,
    pub repo_path: PathBuf,        // main repo root (from ProjectConfig.path)
    pub branch: String,            // new branch name for this session
    pub base_branch: String,       // branch to create from (ProjectConfig.default_branch)
    pub worktree_path: PathBuf,    // target path (from DataPaths::worktree_path)
    pub symlinks: Vec<PathBuf>,    // repo-relative paths to symlink (from ProjectConfig.symlinks)
}

pub struct WorkspaceInfo {
    pub session_id: String,
    pub path: PathBuf,             // absolute path to the worktree/clone root
    pub branch: String,
    pub base_branch: String,       // preserved for PR creation / diff computation
}
```

### Trait Definition

```rust
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

The trait only manages filesystem isolation (worktree/clone creation, symlinks, destruction). Hook execution is the lifecycle engine's responsibility (see Section 4). Implementations store their own path configuration (received via the factory function); the trait signature uses only `session_id` for lookup/destroy operations.

### Worktree Implementation

**`create()`** performs three steps in order:

1. **`git worktree add`**: `git -C {repo_path} worktree add -b {branch} {worktree_path} {base_branch}` via `CommandRunner`.
2. **Create symlinks**: for each entry in `ctx.symlinks`, create `{worktree_path}/{rel_path} → {repo_path}/{rel_path}`. Parent directories created as needed. Each symlink is validated before creation (see Symlink Escape Prevention). If the target doesn't exist, log a warning and skip (non-fatal).
3. **Return `WorkspaceInfo`** with the absolute worktree path, branch name, and base branch.

**`destroy()`** runs `git -C {repo_path} worktree remove --force {worktree_path}`, then deletes the branch. Branch deletion strategy:
- Discover the branch name via `git worktree list --porcelain` matching the worktree path (avoids depending on naming conventions or reading SessionStore).
- Use `git branch -d` (lowercase) by default — refuses to delete unmerged branches, preserving recoverable work.
- Fall back to `git branch -D` (force) only when explicitly requested (e.g., for `killed` sessions where work is definitively abandoned). The lifecycle engine controls this via a parameter or by calling a separate `force_delete_branch()` method.
- If `-d` fails on unmerged work, log a warning and continue — the worktree is removed but the branch is preserved for manual recovery.
- Idempotent — if the worktree or branch is already gone, returns `Ok(())`.

**`exists()`** checks if `worktree_path` exists on disk and appears in `git worktree list --porcelain`. Returns `Result<bool, WorkspaceError>` to surface unexpected filesystem errors.

**`info()`** retrieves `WorkspaceInfo` for a single session by matching `session_id` against `git worktree list --porcelain`. Returns `Ok(None)` if the worktree does not exist.

**`list()`** parses `git worktree list --porcelain` and returns entries whose path is under `worktrees_dir`.

### Symlink Escape Prevention

```rust
fn validate_symlink(
    target_path: &Path,  // {repo}/{rel} — the symlink target
    repo_root: &Path,    // main repo root
) -> Result<(), WorkspaceError> {
    let resolved_target = std::fs::canonicalize(target_path)?;
    let resolved_repo = std::fs::canonicalize(repo_root)?;
    if !resolved_target.starts_with(&resolved_repo) {
        return Err(WorkspaceError::SymlinkEscape {
            target: target_path.to_path_buf(),
            resolved: resolved_target,
            boundary: resolved_repo,
        });
    }
    Ok(())
}
```

Called before creating each symlink. Catches `../../etc/passwd`-style targets that resolve outside the repo root. Note: there is a TOCTOU window between `canonicalize()` and `symlink()` — an attacker could replace a directory with a symlink in between. This is accepted for a local developer tool where the attacker would need write access to the repo directory during the brief window.

Symlinks are **repo-relative** — `symlinks: [".env", ".claude"]` creates `{worktree}/.env → {repo}/.env`. The user configures paths relative to the repo root; the workspace implementation resolves them.

### Factory Registration

```rust
pub fn create_workspace(
    name: &str,
    config: &Config,
    paths: &DataPaths,
) -> Result<Box<dyn Workspace>, PluginError> {
    match name {
        "worktree" => Ok(Box::new(WorktreeWorkspace::new(config, paths))),
        "clone" => Err(PluginError::NotImplemented("clone".into())),
        _ => Err(PluginError::UnknownPlugin(name.into())),
    }
}
```

### PRD Interface Mapping

| PRD Method (FR2) | ADR Mapping |
|------------------|-------------|
| `create()` | `Workspace::create()` |
| `destroy()` | `Workspace::destroy()` |
| `list()` | `Workspace::list()` |
| `exists()` | `Workspace::exists()` |
| `postCreate()` | Subsumed by lifecycle engine hook orchestration (config `afterCreate` + agent `AfterCreate`) |
| `restore()` | Deferred to post-MVP (session restore reuses existing workspace) |

## 3. SessionStore Module

A non-pluggable core module that manages session metadata files and action journals. Not a trait — the PRD specifies flat files as the only persistence backend.

### Shared Status Types

`SessionStatus` and `TerminationReason` are shared enums in `packages/core/src/types/status.rs`, imported by both the SessionStore and the lifecycle engine. This provides compile-time type safety on the system's hottest path (status is read/written every poll tick) without circular dependencies.

```rust
/// Shared status enum — all 16 session statuses from ADR-0001.
#[derive(Debug, Clone, PartialEq, Eq, Display, FromStr)]
#[strum(serialize_all = "snake_case")]
pub enum SessionStatus {
    Spawning, Working, PrOpen, ReviewPending, Approved, Mergeable,
    CiFailed, ChangesRequested, NeedsInput, Stuck,
    Killed, Terminated, Done, Cleanup, Errored, Merged,
}

/// Constrained termination reasons — not a free-form string.
/// Wrapped in Option<TerminationReason> in SessionMetadata.
/// None means "not terminated"; serializes to empty string in KEY=VALUE.
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
```

### Metadata File Format

`sessions/{id}/metadata` — `KEY=VALUE`, one per line. Fixed field set. Values are single-line: the serializer escapes newlines (`\n` → literal `\n`) and backslashes (`\` → `\\`); the deserializer reverses this. The format is human-readable and debuggable with `cat`. Note: do not `source` the file in bash with untrusted values (potential shell injection via `$(...)` or backticks) — use the orchestrator's `read()` API instead.

```bash
SESSION_ID=myproject-42-1
STATUS=working
CREATED_AT=2026-03-07T10:00:00Z
UPDATED_AT=2026-03-07T10:05:00Z
WORKSPACE_PATH=/Users/dev/.agent-orchestrator/a1b2c3d4e5f6-myproject/worktrees/myproject-42-1
AGENT=claude-code
RUNTIME=tmux
ISSUE_ID=42
ATTEMPT=1
BRANCH=
BASE_BRANCH=main
PR_URL=
TOKENS_IN=0
TOKENS_OUT=0
TERMINATION_REASON=
```

Parsing: split each line on the first `=`. Lines starting with `#` are comments (ignored). Empty lines are skipped. Backslash-escaped newlines are unescaped after splitting.

### Journal File Format

`sessions/{id}/journal.jsonl` — one JSON object per line, append-only:

```json
{"action":"merge","target":"PR#123","timestamp":"2026-03-07T10:30:00Z","dedupe_key":"merge:PR#123:1709805000","result":"success","error_code":null,"attempt":1,"actor":"orchestrator"}
{"action":"label","target":"issue#42","timestamp":"2026-03-07T10:30:01Z","dedupe_key":"label:issue#42:1709805001","result":"success","error_code":null,"attempt":1,"actor":"reaction_engine"}
```

### Data Types

```rust
/// Session metadata. Uses custom KEY=VALUE serialization (not serde).
/// Values are single-line with newline escaping.
pub struct SessionMetadata {
    pub session_id: String,
    pub status: SessionStatus,     // typed enum from types/status.rs
    pub created_at: String,        // ISO 8601
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
    pub termination_reason: Option<TerminationReason>,  // None = not terminated
}

#[derive(Serialize, Deserialize)]
pub struct JournalEntry {
    pub action: String,
    pub target: String,
    pub timestamp: String,        // ISO 8601
    pub dedupe_key: String,       // "{action}:{target}:{timestamp_window}"
    pub result: JournalResult,
    pub error_code: Option<String>,
    pub attempt: u32,
    pub actor: String,            // "orchestrator" | "reaction_engine" | "human"
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalResult {
    Success,
    Failed,
    Skipped,
}
```

### SessionStore API

```rust
pub struct SessionStore {
    paths: DataPaths,
}

impl SessionStore {
    pub fn new(paths: DataPaths) -> Self;

    /// Create a new session. Uses atomic directory creation for race-free reservation.
    /// Fails with StoreError::SessionExists if session directory already exists.
    pub fn create(&self, initial: &SessionMetadata) -> Result<(), StoreError>;

    /// Read metadata for a session. Validates status/termination_reason on parse.
    pub fn read(&self, session_id: &str) -> Result<SessionMetadata, StoreError>;

    /// Write metadata atomically (temp file + fsync + rename).
    pub fn write(&self, session_id: &str, metadata: &SessionMetadata) -> Result<(), StoreError>;

    /// Append a journal entry. Opens file in append mode, writes one JSONL line, fsyncs.
    pub fn append_journal(&self, session_id: &str, entry: &JournalEntry) -> Result<(), StoreError>;

    /// Read all journal entries for a session.
    /// Skips malformed trailing lines (with warning log) for crash resilience.
    pub fn read_journal(&self, session_id: &str) -> Result<Vec<JournalEntry>, StoreError>;

    /// Archive session: rename sessions/{id}/ → archive/{id}-{YYYYMMDD}T{HHMMSS}Z/.
    pub fn archive(&self, session_id: &str) -> Result<(), StoreError>;

    /// List all active (non-archived) sessions.
    pub fn list(&self) -> Result<Vec<SessionMetadata>, StoreError>;

    /// Check if a session exists (non-archived).
    pub fn exists(&self, session_id: &str) -> Result<bool, StoreError>;
}
```

### Atomic Write

```rust
fn atomic_write(path: &Path, content: &[u8]) -> Result<(), StoreError> {
    let tmp = path.with_extension("tmp");
    let mut file = File::create(&tmp)?;
    file.write_all(content)?;
    file.sync_all()?;              // fsync before rename
    std::fs::rename(&tmp, path)?;  // atomic on POSIX
    Ok(())
}
```

Note: full power-loss durability would require an additional `fsync` on the parent directory after `rename()`. This is deferred — the current approach provides safety against process crashes (SIGKILL, panic), which are the realistic failure mode on developer machines.

### Race-Free Creation

```rust
fn create_session_dir(path: &Path) -> Result<(), StoreError> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            Err(StoreError::SessionExists(path.to_path_buf()))
        }
        Err(e) => Err(e.into()),
    }
}
```

`create_dir` is atomic on POSIX — two concurrent creators get one success and one `AlreadyExists`. This is the directory-level equivalent of `O_EXCL` on file creation.

### Session ID Validation (Path Traversal Prevention)

```rust
fn validate_session_id(id: &str) -> Result<(), StoreError> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
        || id == "."
        || id == ".."
        || id.starts_with('.')
    {
        return Err(StoreError::InvalidSessionId(id.to_string()));
    }
    Ok(())
}
```

Every public `SessionStore` method calls this before constructing paths. Defense in depth alongside the session ID format validation from ADR-0004 (`[a-zA-Z0-9._-]`).

### Archive on Delete

`archive()` renames `sessions/{id}/` → `archive/{id}-{YYYYMMDD}T{HHMMSS}Z/` (UTC, ISO 8601 basic format). The timestamp suffix prevents collisions when the same session ID is reused across attempts. Archives are read-only after creation. Note: `sessions/` and `archive/` must reside on the same filesystem (both are under `{root}/`); `rename()` would fail with `EXDEV` across mount points.

## 4. Lifecycle Engine Orchestration

How the lifecycle engine coordinates workspace, metadata, and hooks during session lifecycle events.

### Session Creation Sequence

```
 1. Engine receives spawn request (issue_id, project_config)
 2. Compute session_id: {prefix}-{issueId}-{attempt}
 3. session_store.create(initial_metadata)          // race-free reservation
 4. workspace.create(WorkspaceCreateContext)         // git worktree + symlinks
 5. session_store.write() with WORKSPACE_PATH       // record worktree path + base_branch
 6. Run config hook: afterCreate                    // ProjectConfig.hooks.after_create
 7. Run agent hooks: AfterCreate                    // Agent::workspace_hooks()
 8. Install agent PostToolUse hooks                 // write script files into workspace
 9. Run config hook: beforeRun                      // before first attempt
10. Set env: AO_SESSION={session_id}, AO_DATA_DIR={session_dir}
11. Execute LaunchPlan via Runtime                  // ADR-0004 plan execution
```

**Unwind on failure (steps 2-9):** if any step in the pre-launch sequence fails, the engine unwinds all prior steps. Workspace destruction uses `DataPaths::worktree_path(session_id)` directly — not the `WORKSPACE_PATH` field from metadata, since step 5 (which records it) may not have completed. Metadata is archived with `TERMINATION_REASON=SpawnFailed`. Step 10 (env setup) cannot fail. Step 11 (plan execution) failures are handled by ADR-0004's plan failure semantics (transition to `errored`, destroy runtime).

### Session Destruction Sequence

```
1. Run config hook: beforeRemove       // failure logged, does not block destruction
2. workspace.destroy(session_id)       // git worktree remove + conditional branch deletion
3. session_store.archive(session_id)   // move to archive/
```

Called from lifecycle engine entry actions for terminal states (`cleanup`, `killed`, etc.). ADR-0001 lists `destroyWorkspace()` as an entry action; archival is an additional step owned by this ADR. `beforeRemove` hook failures are logged but do not prevent destruction — resource cleanup must be best-effort to avoid leaked workspaces.

### Hook Execution

```rust
pub struct HookContext {
    pub session_id: String,
    pub workspace_path: PathBuf,
    pub repo_path: PathBuf,
    pub data_dir: PathBuf,         // session metadata dir
}

/// Default hook timeout: 60 seconds.
/// Per-hook timeout configuration is a post-MVP extension for ADR-0003's Hooks struct.
pub const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(60);

/// Run a shell command hook with timeout.
pub async fn run_hook(
    command: &str,
    ctx: &HookContext,
    timeout: Duration,
) -> Result<(), HookError>;
```

Hooks run via `CommandRunner` with `cwd` set to the workspace path. Environment variables injected: `AO_SESSION`, `AO_DATA_DIR`, `AO_WORKSPACE_PATH`, `AO_REPO_PATH`. Hooks exceeding `timeout` are killed via SIGTERM then SIGKILL.

**Hook ordering:**
- Creation: config `afterCreate` → agent `AfterCreate` hooks (agent hooks after config hooks per ADR-0004)
- Each attempt: config `beforeRun` → [agent runs] → config `afterRun`
- Destruction: config `beforeRemove` → workspace destroy → archive

Note: `AgentWorkspaceHook::PostToolUse` (ADR-0004) is a file-based hook installed into the workspace at creation time (step 8), not a lifecycle event hook. It is triggered by the agent runtime during execution (e.g., Claude Code invokes it after each tool use).

### Metadata Updates During Poll Loop

Per ADR-0001's three-phase poll loop:

- **Gather phase**: `session_store.read()` to get current metadata; `Agent::parse_session_info()` extracts fresh `AgentSessionInfo` (branch, pr_url, tokens_in, tokens_out)
- **Evaluate phase**: pure graph walk, no metadata I/O
- **Transition phase**: `session_store.write()` with updated status, tokens, branch, pr_url; `session_store.append_journal()` for each entry action executed

## 5. Module Structure

```
packages/core/src/
├── types/
│   └── status.rs           # SessionStatus, TerminationReason (shared enums)
├── workspace/
│   ├── mod.rs              # Workspace trait, WorkspaceCreateContext, WorkspaceInfo, factory
│   ├── worktree.rs         # WorktreeWorkspace implementation
│   └── error.rs            # WorkspaceError
├── store/
│   ├── mod.rs              # SessionStore, DataPaths
│   ├── metadata.rs         # SessionMetadata, KEY=VALUE serialization with newline escaping
│   ├── journal.rs          # JournalEntry, JSONL serialization
│   └── error.rs            # StoreError
└── hooks.rs                # HookContext, run_hook(), DEFAULT_HOOK_TIMEOUT
```

## 6. Testing Strategy

### Unit Tests

- **DataPaths**: verify all path computations, `.origin` collision detection, `ensure_dirs()` idempotency, `.origin` missing with non-empty sessions (should error)
- **SessionStore**: create/read/write/archive round-trips, `O_EXCL` race semantics (concurrent creates in threads), atomic write crash simulation (check no partial writes), session ID validation (path traversal attempts), KEY=VALUE serialization edge cases (values with `=`, empty values, Unicode, newlines, backslashes, shell-injection characters like `$()` and backticks)
- **SessionStatus/TerminationReason**: Display/FromStr round-trip for all variants, invalid string parsing returns error
- **JournalEntry**: JSONL serialization round-trip, append-then-read consistency, malformed trailing line skipped with warning
- **Symlink validation**: escape attempts (`../../../etc/passwd`), valid repo-relative paths, missing targets (warning, not error)
- **Session ID validation**: all forbidden patterns rejected, valid tmux-safe IDs accepted

### Integration Tests

- **WorktreeWorkspace**: create worktree → verify branch exists → verify symlinks → destroy → verify cleanup. Requires a real git repo (created in temp dir). Test conditional branch deletion: unmerged branch preserved with warning on `git branch -d` failure.
- **Hook execution**: run a shell command that writes to a file → verify file exists → verify env vars injected → verify timeout kills long-running hooks → verify `beforeRemove` failure does not block destruction
- **Full lifecycle**: create session metadata → create workspace → run hooks → update metadata → archive → verify archive contents

### Property Tests

- **Metadata round-trip**: arbitrary `SessionMetadata` values (including values with newlines, backslashes, `=`, Unicode) serialize to KEY=VALUE and deserialize back identically
- **Session ID validation**: no valid session ID produces a path outside `sessions_dir`

## 7. Deferred to Post-MVP

| Feature | Deferred To | Reason |
|---------|-------------|--------|
| `clone` workspace plugin | Post-MVP | Worktree covers primary use case |
| `Workspace::restore()` | FR6 (CLI) | Session restore reuses existing workspace without re-creating |
| Archive garbage collection | Post-MVP | Archives accumulate; periodic cleanup is a convenience feature |
| Journal compaction | Post-MVP | JSONL files grow unbounded; not a concern at MVP session volumes |
| Cross-session atomic updates | Post-MVP | No transactions spanning multiple session files (acknowledged limitation) |
| Symlink for directories (e.g., `node_modules/.cache`) | MVP | Supported — symlink creation handles both files and directories |
| Hot-reload of hooks config | Post-MVP (ADR-0003) | Hooks read from config at session creation time |
| Per-hook timeout config | Post-MVP | Default 60s hardcoded; per-hook `timeout` field in `Hooks` struct deferred |
| Parent directory fsync | Post-MVP | Full power-loss durability; process crash safety is sufficient for MVP |
| Journal write serialization | FR4 (Reactions) | Concurrent appends from reaction engine will need per-session mutex or channel; MVP has single writer per session |