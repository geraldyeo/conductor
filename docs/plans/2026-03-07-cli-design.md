# CLI Design Document

## Overview

This document details the design for ADR-0007 (CLI), covering the `ao` binary — the user-facing surface that ties together the session lifecycle engine (ADR-0001), configuration system (ADR-0003), plugin system (ADR-0004), workspace and session metadata (ADR-0005), and tracker integration (ADR-0006).

The CLI serves two audiences: humans interacting via terminal, and scripts/supervisor agents consuming structured JSON output.

## Key Design Decisions

1. **`clap` v4 with derive macros** — locked in by ADR-0002.
2. **Foreground process model** — `ao start` blocks, logs to stdout. No self-daemonizing.
3. **Auto-resolve project** — single-project configs just work; `-p` flag for multi-project.
4. **IPC via Unix domain socket** — mutating commands delegate to the running orchestrator. Read-only commands access `SessionStore` directly.
5. **`--json` is global** — all commands support JSON output for scripting.
6. **Orchestrator-as-session deferred** — `ao start` at MVP = poll loop only. Highest priority post-MVP.

## Command Structure

```
ao
├── init              # Generate agent-orchestrator.yaml
├── start             # Start orchestrator (poll loop, IPC listener)
├── stop              # Stop orchestrator via IPC
├── status            # Show session table (read-only, no IPC needed)
├── spawn             # Spawn session for an issue
├── batch-spawn       # Spawn sessions for multiple issues
├── send              # Send message to a session
└── session
    ├── ls            # List sessions (alias for status with session focus)
    ├── kill          # Kill a session
    └── cleanup       # Kill sessions with terminal tracker state
```

### Global Flags

| Flag | Type | Default | Purpose |
|------|------|---------|---------|
| `--config <path>` | `Option<PathBuf>` | `None` | Override config discovery (sets `AO_CONFIG_PATH` equivalent) |
| `--project <id>` / `-p` | `Option<String>` | `None` | Override project auto-resolution |
| `--json` | `bool` | `false` | JSON output for all commands |
| `--verbose` / `-v` | `bool` | `false` | Debug-level logging to stderr |

### Project Auto-Resolution

```
1. If --project given → use it (error if not in config)
2. If config has exactly 1 project → use it
3. If CWD is inside a project's `path` → use that project
4. Otherwise → error with list of available projects
```

Step 3 uses the `path` field from `ProjectConfig` (ADR-0003), canonicalized at config load time.

## IPC Control Plane

The orchestrator (`ao start`) listens on a Unix domain socket. Mutating commands connect as clients.

### Socket Location

`~/.agent-orchestrator/orchestrator.sock` (orchestrator-wide, not per-project)

### Protocol

Length-prefixed JSON over Unix domain socket. Simple request-response: client sends a request, orchestrator sends a response, connection closes. No persistent connections, no streaming.

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorRequest {
    Spawn { project_id: String, issue_id: String, agent: Option<String>, open: bool },
    BatchSpawn { project_id: String, issue_ids: Vec<String>, agent: Option<String>, open: bool },
    Send { session_id: String, content: String, no_wait: bool, timeout_secs: u64 },
    Kill { session_id: String },
    Cleanup { project_id: String, dry_run: bool },
    Stop,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorResponse {
    Ok { message: String },
    SpawnResult { session_id: String, branch: String, workspace_path: String },
    BatchSpawnResult { results: Vec<BatchSpawnItem> },
    CleanupResult { killed: Vec<String>, skipped: Vec<String> },
    SendResult { delivered: bool, activity_state: String },
    Error { code: String, message: String },
}

#[derive(Serialize, Deserialize)]
pub struct BatchSpawnItem {
    pub issue_id: String,
    pub outcome: BatchSpawnOutcome,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum BatchSpawnOutcome {
    Spawned { session_id: String, branch: String },
    Skipped { reason: String },
    Failed { error: String },
}
```

### Why Unix Domain Socket

- Lower overhead than HTTP, no port conflicts
- File-permission-based access control
- Socket file doubles as "is the orchestrator running?" check
- No HTTP semantics needed for local CLI-to-daemon IPC

### Lifecycle

- `ao start` creates the socket, listens for connections, runs the poll loop concurrently (both on the same tokio runtime).
- `ao stop` connects, sends `Stop`, orchestrator drains active poll tick, removes socket file, exits.
- If socket file exists but connection refused → stale socket from a crashed orchestrator. CLI commands detect this, delete the stale file, and print "orchestrator not running."

### Command Routing

| Command | IPC? | Rationale |
|---------|------|-----------|
| `ao init` | No | Config generation, no orchestrator needed |
| `ao start` | No (is the server) | Creates the socket |
| `ao stop` | Yes | Sends `Stop` request |
| `ao status` | No | Reads SessionStore files directly |
| `ao spawn` | Yes | Orchestrator owns session creation sequence |
| `ao batch-spawn` | Yes | Delegates to orchestrator's spawn logic |
| `ao send` | Yes (fallback: direct) | Orchestrator owns activity detection + delivery |
| `ao session ls` | No | Reads SessionStore files directly |
| `ao session kill` | Yes | Sets `manualKill` in PollContext |
| `ao session cleanup` | Yes | Needs tracker + kill coordination |

Read-only commands work without the orchestrator (they load config, construct `DataPaths` per project, and iterate `SessionStore::list()` — lightweight, no plugin construction). Mutating commands require the orchestrator, except `ao send --no-wait` which falls back to a direct `tmux send-keys` call via `CommandRunner` (no `Runtime` construction — tmux is the only MVP runtime, and `send-keys` only needs the session name from `SessionMetadata`). A warning is printed.

## Command Implementations

### `ao init`

```
ao init [--auto] [--smart] [-o <path>]
```

- **Interactive mode (default):** Prompts for project ID, repo (`owner/repo`), path, agent, runtime. Validates inputs inline. Writes to CWD or `-o <path>`.
- **`--auto`:** No prompts. Infers project ID from directory name, repo from `git remote get-url origin`, path from CWD. Errors if inference fails.
- **`--smart`:** Extends `--auto` — analyzes project for language, framework, test runner, package manager. Generates `agentRules` with detected conventions. At MVP, behaves like `--auto` with a note.
- **Guard:** Refuses to overwrite an existing config file unless `-o` specifies a different path.
- **No IPC.** Calls `config::generate_default()` (ADR-0003).

### `ao start`

```
ao start [--no-dashboard]
```

Startup sequence:

1. `config::load()` — discover + load + validate (ADR-0003)
2. Validate plugins — `create_tracker()`, `create_runtime()`, `create_workspace()` for each project. Fail-fast on missing `gh`, bad auth, etc.
3. `DataPaths::ensure_dirs()` for each project — collision detection via `.origin` file (ADR-0005)
4. Create `SessionStore` per project
5. Bind Unix domain socket at `{root}/orchestrator.sock` — error if already bound (another instance running)
6. Load non-terminal sessions from `SessionStore::list()` — crash recovery (ADR-0001)
7. Run one immediate poll tick for crash recovery
8. Start poll loop (30s default interval) + IPC listener on tokio runtime
9. Print "Orchestrator running. Press Ctrl-C to stop." (or JSON `{"status":"started"}`)

Graceful shutdown (Ctrl-C / SIGTERM / `ao stop`):

1. Stop accepting new IPC connections
2. Wait for current poll tick to complete (bounded by tick timeout)
3. Remove socket file
4. Exit 0

`--no-dashboard` is accepted but no-op at MVP (forward compatibility for FR7).

The socket file serves as the liveness indicator — no PID file needed.

### `ao stop`

```
ao stop
```

Connects to IPC socket, sends `Stop`, waits for acknowledgment, exits. If socket doesn't exist or connection refused → "Orchestrator is not running" (exit 0, idempotent).

### `ao spawn`

```
ao spawn <issue> [--agent <name>] [--open]
```

Project resolved via global `-p` flag or auto-resolution. `<issue>` is the tracker issue ID.

Sends `Spawn` request to orchestrator. The orchestrator executes the full creation sequence:

1. Pre-spawn validation: `tracker.get_issue()` + `classify_state()` (ADR-0006)
2. Compute session ID: `{prefix}-{issueId}-{attempt}` (ADR-0004)
3. Derive branch: `tracker.branch_name()` (ADR-0006)
4. SessionStore create → Workspace create → hooks → LaunchPlan execute (ADR-0005 steps 1-10)
5. Post-spawn workpad comment (ADR-0006, non-blocking)

CLI prints: session ID, branch name, workspace path. With `--open`: attach terminal via `Runtime::attach_info()`.

Duplicate detection: orchestrator filters `SessionStore::list()` for non-terminal sessions whose `session_id` starts with `{prefix}-{issueId}-` (trailing hyphen prevents `myproj-4` matching `myproj-42`). If found, errors with the existing session ID. Users must `ao session kill` first.

### `ao batch-spawn`

```
ao batch-spawn <issues...> [--agent <name>] [--open]
```

Sends `BatchSpawn` to orchestrator. Orchestrator iterates issues:

- Dedup against existing sessions AND within the batch
- Pre-flight checks (plugin validation) run once before the loop
- 500ms delay between spawns (PRD spec)
- Collects results per issue: `spawned`, `skipped`, or `failed` with reason

Failures don't abort the batch — remaining issues still spawn.

### `ao send`

```
ao send <session> [message...] [-f <file>] [--no-wait] [--timeout <secs>]
```

Message source priority: `-f <file>` > positional args > stdin (if not a TTY).

**With orchestrator running (IPC):** Sends `Send` request. Orchestrator handles:

1. Look up session's Agent + Runtime instances
2. Poll `detect_activity()` until idle (or timeout, default 600s)
3. Clear partial input: `Runtime::execute_step(SendMessage { content: "\x15" })` (Ctrl-U)
4. Deliver: `SendMessage` for short messages, `SendBuffer` for large (>1KB, per PRD tmux buffer strategy)
5. Verify delivery: 3 retries checking activity state transitions to `active`
6. Return `SendResult { delivered, activity_state }`

**`--no-wait`:** Skip activity polling, deliver immediately.

**Without orchestrator (fallback):** Direct `tmux send-keys` via `CommandRunner` (no `Runtime` construction — tmux is the only MVP runtime). No activity check. Warning: "Orchestrator not running, delivering without busy detection."

### `ao status`

```
ao status [-p <project>] [--json]
```

No IPC. Reads `SessionStore::list()` directly. Displays:

```
SESSION          STATUS     ISSUE  BRANCH              AGE   TOKENS
myproj-42-1      working    #42    42-fix-login-bug    2h    12.4k in / 8.1k out
myproj-55-1      pr_open    #55    55-add-search       45m   28.0k in / 15.2k out
myproj-71-1      killed     #71    71-refactor-auth    3h    5.2k in / 3.0k out
```

Columns: session ID, status, issue, branch, age (from `created_at`), tokens (human-friendly with k suffix).

With `--json`: array of `SessionMetadata` objects serialized via serde.

### `ao session ls`

```
ao session ls [-p <project>]
```

Alias for `ao status`. Same output, same implementation. Exists for discoverability.

### `ao session kill`

```
ao session kill <session>
```

Sends `Kill` request to orchestrator. Orchestrator sets `manualKill = true` in the session's `PollContext` (ADR-0001). The next poll tick evaluates the global edge at precedence 0 → transitions to `killed` → entry action destroys runtime + workspace.

Not immediate: the poll loop's transition phase handles cleanup (runtime destroy, workspace destroy, metadata update, journal append). After setting the flag, the CLI briefly polls `SessionStore` (up to 5s, checking every 500ms) for the session to reach `killed` status. If confirmed within 5s, prints "Session killed." Otherwise prints "Kill scheduled, will complete within {poll_interval}s."

Without orchestrator: error — "Orchestrator not running. Cannot kill session."

### `ao session cleanup`

```
ao session cleanup [-p <project>] [--dry-run]
```

Sends `Cleanup` request to orchestrator. Orchestrator:

1. `SessionStore::list()` → filter non-terminal sessions
2. For each: `tracker.get_issue()` + `classify_state()`
3. If terminal → set `manualKill = true`
4. Return list of killed and skipped sessions

`--dry-run`: runs steps 1-2, reports what would be killed, doesn't set the flag.

## Error Handling & Exit Codes

### Exit Codes

| Code | Meaning | Examples |
|------|---------|---------|
| 0 | Success | Command completed, `ao stop` when already stopped (idempotent) |
| 1 | General error | Spawn failed, send timeout, IPC error |
| 2 | Usage error | Invalid args, missing required arg (clap default) |
| 3 | Config error | Config not found, validation failed, unknown project |
| 4 | Orchestrator not running | Mutating command sent but no orchestrator to handle it |

### Error Display

Human mode (default):

```
error: issue #42 is closed (terminal state)
  → Cannot spawn a session for a terminal issue.
  → Check issue status: https://github.com/owner/repo/issues/42
```

Pattern: `error:` prefix, one-line summary, indented context lines with `→`.

JSON mode (`--json`):

```json
{"error":{"code":"issue_terminal","message":"issue #42 is closed (terminal state)","issue_id":"42","issue_url":"https://github.com/owner/repo/issues/42"}}
```

### Output Conventions

| Stream | Human mode | JSON mode |
|--------|-----------|-----------|
| stdout | Tables, status messages, results | JSON objects (one per result) |
| stderr | `error:`, `warning:`, debug logs (`-v`) | Same (logs never pollute JSON stdout) |

Progress indicators (e.g., `ao send` polling for idle) print to stderr. In JSON mode, no progress — just the final result on stdout.

`warning:` prefix for non-fatal issues. Warnings don't affect exit code.

## Module Structure

```
packages/cli/src/
├── main.rs              # Entry point, clap parse, dispatch
├── commands/
│   ├── mod.rs           # Command enum (clap Subcommand derive)
│   ├── init.rs          # ao init
│   ├── start.rs         # ao start (poll loop + IPC listener)
│   ├── stop.rs          # ao stop
│   ├── status.rs        # ao status + ao session ls
│   ├── spawn.rs         # ao spawn + ao batch-spawn
│   ├── send.rs          # ao send
│   └── session.rs       # ao session kill, ao session cleanup
├── ipc/
│   ├── mod.rs           # OrchestratorRequest, OrchestratorResponse types
│   ├── server.rs        # Unix socket listener (used by ao start)
│   └── client.rs        # Unix socket client (used by other commands)
├── output/
│   ├── mod.rs           # OutputMode enum (Human | Json)
│   ├── table.rs         # Human-readable table formatting
│   └── json.rs          # JSON serialization helpers
├── resolve.rs           # Project auto-resolution logic
└── error.rs             # CliError, exit code mapping
```

### Orchestrator in Core

The orchestrator loop lives in `packages/core/src/orchestrator/`, not in `cli`. The `ao start` command constructs an `Orchestrator` struct from `core` and calls `orchestrator.run()`.

```rust
// packages/core/src/orchestrator/mod.rs
pub struct Orchestrator {
    config: Arc<Config>,
    stores: HashMap<String, SessionStore>,
    plugins: HashMap<String, ProjectPlugins>,
    socket_path: PathBuf,
    ipc_tx: mpsc::Sender<(OrchestratorRequest, oneshot::Sender<OrchestratorResponse>)>,
    ipc_rx: mpsc::Receiver<(OrchestratorRequest, oneshot::Sender<OrchestratorResponse>)>,
}
```

**Concurrency model — channel-based serialization.** `run()` spawns two concurrent tasks: the IPC listener and the poll loop. The IPC listener sends requests via `mpsc` to the poll loop task, which drains the channel between ticks. Each request includes a `oneshot::Sender` for the response. This serializes all mutations without locks — no `Mutex`, no `RwLock`, no lock ordering.

```
IPC listener task                     Poll loop task
─────────────────                     ──────────────
accept connection ──→ mpsc::send ──→  drain channel (between ticks)
wait on oneshot   ←── oneshot::send ←── handle request, mutate state
send response                         continue to next tick
```

The `shutdown` watch channel unifies `ao stop` (IPC) and Ctrl-C (`tokio::signal::ctrl_c()`) into a single shutdown trigger. SIGTERM is handled via `tokio::signal::unix::signal(SignalKind::terminate())`. SIGHUP is reserved for post-MVP config hot-reload.

## Integration Points

| CLI Command | ADR-0001 | ADR-0003 | ADR-0004 | ADR-0005 | ADR-0006 |
|-------------|----------|----------|----------|----------|----------|
| `ao init` | — | `generate_default()` | — | — | — |
| `ao start` | Poll loop, crash recovery | `load()` | Plugin factories | `SessionStore::list()`, `DataPaths::ensure_dirs()` | `create_tracker()` validation |
| `ao stop` | Drain current tick | — | — | — | — |
| `ao spawn` | — | — | `create_agent()`, `create_runtime()`, `LaunchPlan` | Session creation steps 1-10 | Pre-spawn validation, `branch_name()`, workpad comment |
| `ao send` | — | — | `Agent::detect_activity()`, `Runtime::execute_step(SendMessage)` | — | — |
| `ao status` | — | — | — | `SessionStore::list()` | — |
| `ao session kill` | `manualKill` PollContext flag | — | — | — | — |
| `ao session cleanup` | `manualKill` PollContext flag | — | — | `SessionStore::list()` | `get_issue()`, `classify_state()` |

## Deferred Items

| Feature | Deferred to | Reason |
|---------|-------------|--------|
| Orchestrator-as-session | Post-MVP (highest priority) | Depends on ADR-0008 (prompt system) |
| `ao session restore` | Post-MVP | `RestorePlan` is post-MVP (ADR-0004) |
| `ao review-check` | Post-MVP | Requires GraphQL PR review integration |
| `ao dashboard` / `ao open` | FR7/FR8 | Separate FRs |
| `--smart` flag on `ao init` | Post-MVP | Project analysis heuristics |
| `ao start <url>` one-command onboarding | Post-MVP | FR14 clone + detect + init + start pipeline |
| `--daemon` flag on `ao start` | Post-MVP | Foreground is sufficient at MVP |
| Shell completions generation | Post-MVP | `clap_complete` is additive |
| Hot-reload config (SIGHUP or file watch) | Post-MVP | Matches ADR-0003 deferral |
| `ao config check` linting command | Post-MVP | ADR-0003 deferred this to FR6 |
| `--no-orchestrator` on `ao start` | Post-MVP | MVP always runs without orchestrator agent; flag becomes meaningful when orchestrator-as-session lands |
| `--rebuild` on `ao start` | Post-MVP | Relevant to dashboard rebuild (FR7) |

## Testing Strategy

### Unit Tests

- **Project auto-resolution:** test all 4 branches (explicit flag, single project, CWD match, ambiguous)
- **IPC serialization:** round-trip `OrchestratorRequest`/`OrchestratorResponse` through serde
- **Error formatting:** assert human-mode and JSON-mode error output
- **Table formatting:** assert column alignment, token formatting (k suffix), age display

### Integration Tests

- **`ao init`:** generate config, assert it round-trips through `config::load()`
- **IPC round-trip:** start server in test, send request from client, assert response
- **`ao spawn` → `ao status`:** spawn a session (with mock tracker/runtime), verify it appears in status output
- **`ao session kill`:** spawn, kill via IPC, verify `manualKill` set in PollContext
- **Stale socket detection:** create a socket file, attempt connect, verify cleanup and error message
- **`ao send` fallback:** send without orchestrator running, verify direct delivery with warning

### Manual Tests

- End-to-end with real tmux and Claude Code agent
- Multi-project config with `-p` flag switching
- `ao batch-spawn` with duplicate detection
