# Walking Skeleton — Design

**Date:** 2026-03-08
**Status:** Accepted
**Scope:** MVP walking skeleton — `ao spawn` end-to-end against a real GitHub issue

## Goal

Prove the full stack integrates before building out remaining MVP features. A working `ao spawn` command exercises every architectural layer: CLI → IPC → Orchestrator → Tracker → Workspace → PromptEngine → Agent → Runtime → SessionStore.

Full MVP (all 10 commands, full poll cycle, all 16 state transitions) is a subsequent phase.

## Approach

Strict bottom-up, following the dependency graph. Parallelism at the infrastructure layer (M2, M3) where file ownership is non-overlapping. 1–2 parallel worktrees maximum.

## Task Breakdown

### M1 — Scaffold + shared types
*Must go first. Everything depends on it.*

- Cargo workspace at repo root: `packages/cli`, `packages/core`
- Shared `Cargo.toml` with workspace dependencies (`tokio`, `serde`, `serde_json`, `clap`, `tracing`, `anyhow`, `garde`, `tera`, `async-trait`)
- `packages/core/src/types/`:
  - `SessionStatus` enum (16 statuses)
  - `TerminationReason` enum
  - `RuntimeStep` enum
  - `LaunchPlan` struct (`Vec<RuntimeStep>`)
  - `AgentInfo` struct
  - `IssueContent` struct
  - `TrackerState` enum (`Active` / `Terminal`)
- `packages/core/src/lib.rs` — module declarations (stubs)
- Gate: `cargo build --workspace` passes

### M2a — CommandRunner + DataPaths
*Parallel with M2b. After M1.*

- `packages/core/src/utils/command_runner.rs` — async subprocess wrapper using `tokio::process::Command`; captures stdout/stderr; configurable timeout; returns structured result
- `packages/core/src/utils/data_paths.rs` — `DataPaths` struct; computes paths under `~/.agent-orchestrator/{hash}/sessions/`, `.../worktrees/`, `.../archive/`
- Unit tests with real subprocesses (echo, false) and temp directories

### M2b — SessionStore
*Parallel with M2a. After M1. No dependency on M2a.*

- `packages/core/src/session_store/` — core module, not pluggable
- Atomic write: write to `.tmp`, fsync, rename
- Race-free creation: `create_dir` (fails if exists)
- JSONL journal: append with fsync
- Malformed trailing-line resilience on read
- Unit tests with temp directories

### M3a — Worktree Workspace
*Parallel with M3b. After M2a.*

- `packages/core/src/plugins/workspace/worktree.rs` — `WorktreeWorkspace`
- Implements `Workspace` trait: `create()`, `destroy()`, `exists()`, `info()`, `list()`
- Symlink escape prevention on all path operations
- `.origin` collision detection with non-empty-sessions guard
- Conditional branch deletion (`-d` default, `-D` for abandoned)
- Uses `CommandRunner`

### M3b — GitHub Tracker
*Parallel with M3a. After M2a.*

- `packages/core/src/plugins/tracker/github.rs` — `GitHubTracker`
- Implements `Tracker` trait (5 MVP methods): `get_issue()`, `branch_name()`, `issue_url()`, `get_issue_content()`, `add_comment()`
- `classify_state()` pure function outside the trait; unmatched states default to `Active`
- `validate()` at factory construction for fail-fast
- Parses `gh --json` structured output via `CommandRunner`
- Unit tests with fixture JSON

### M4 — PromptEngine
*After M1. Can run parallel to M3a/M3b if a slot is free.*

- `packages/core/src/prompt/` — `PromptEngine` struct (stateless after construction)
- Methods: `render_launch()`, `render_continuation()` (stub), `render_orchestrator()` (stub)
- 5-layer composition: base + context + skills + rules + tools (tools empty at MVP)
- Tera template engine; templates in `packages/core/templates/layers/`
- `sanitize_issue_content()` — select N recent comments, truncate, escape fence delimiters (`<comment ` with trailing space), reverse to chronological
- `load_skills()` from `{project.path}/.ao/skills/*.md`, sorted alphabetically
- `load_user_rules()` with symlink escape prevention
- Unit tests for sanitization and rendering

### M5 — ClaudeCode Agent + Tmux Runtime
*After M1 + M2a.*

- `packages/core/src/plugins/agent/claude_code.rs` — `ClaudeCodeAgent`
  - `launch_plan()` — produces `Vec<RuntimeStep>` for tmux session creation + Claude invocation
  - `detect_activity(GatherContext)` — checks tmux pane output for activity signals
  - `parse_session_info(GatherContext)` — extracts cost/token data from output
- `packages/core/src/plugins/runtime/tmux.rs` — `TmuxRuntime`
  - `execute_step(RuntimeStep)` — dispatches to tmux subcommands via `CommandRunner`
  - `get_output()`, `is_alive()`, `destroy()`, `supported_steps()`
- Unit tests with real tmux where available; integration tests stubbed

### M6 — Lifecycle Spawn Sequence
*After M2b, M3a, M3b, M4, M5.*

- `packages/core/src/orchestrator/` — `Orchestrator` struct
- Implements the 8-step spawn sequence:
  1. Validate issue exists and is not terminal (Tracker)
  2. Create session record (SessionStore)
  3. Create workspace (WorktreeWorkspace)
  4. Run `afterCreate` hook
  5. Run `beforeRun` hook
  6. Render launch prompt (PromptEngine)
  7. Build LaunchPlan (ClaudeCodeAgent)
  8. Execute LaunchPlan (TmuxRuntime)
- Unwind boundary: steps 2–8 (destroy workspace + session on failure)
- Transitions session status through `spawning → working`
- Integration test: full spawn sequence with stubbed tracker, real tmux, temp workspace

### M7 — CLI + IPC + `ao spawn`
*After M6.*

- `packages/cli/src/main.rs` — `clap`-based CLI entry point; `--json` global flag
- `ao spawn <issue-url>` subcommand
- `packages/core/src/ipc/` — Unix domain socket at `~/.agent-orchestrator/orchestrator.sock`; mpsc channel from IPC listener to orchestrator event loop
- End-to-end test: invoke `ao spawn` → IPC → orchestrator → spawn sequence

## File Ownership Map

| Task | Owns |
|------|------|
| M1 | `Cargo.toml`, `packages/core/src/types/`, `packages/core/src/lib.rs`, `packages/cli/Cargo.toml` |
| M2a | `packages/core/src/utils/command_runner.rs`, `packages/core/src/utils/data_paths.rs` |
| M2b | `packages/core/src/session_store/` |
| M3a | `packages/core/src/plugins/workspace/` |
| M3b | `packages/core/src/plugins/tracker/` |
| M4 | `packages/core/src/prompt/`, `packages/core/templates/` |
| M5 | `packages/core/src/plugins/agent/`, `packages/core/src/plugins/runtime/` |
| M6 | `packages/core/src/orchestrator/` |
| M7 | `packages/cli/src/`, `packages/core/src/ipc/` |

## Execution Sequence (1–2 parallel slots)

```
Step 1: M1
Step 2: M2a + M2b (parallel)
Step 3: M3a + M3b (parallel)
Step 4: M4 (or M4 + M5 if M3 completes early)
Step 5: M5
Step 6: M6
Step 7: M7
```

## Review Gate

Each task runs `/code-review-multi diff` before opening a PR. No Critical findings may remain. Warnings after 2 rounds are documented in the PR with rationale.

## Success Criteria

`ao spawn <github-issue-url>` creates a tmux session running Claude Code against the issue in an isolated git worktree, with session metadata persisted to `~/.agent-orchestrator/`.
