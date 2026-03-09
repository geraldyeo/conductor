# Conductor

Scalable orchestration layer for AI coding agents. Conductor manages parallel, isolated agent sessions — handling workspace creation, lifecycle monitoring, multi-turn execution, priority-based dispatch, and cost tracking across agents like Claude Code, Aider, Codex, and others.

## Status

**Full MVP complete.** All 10 CLI commands, the poll-driven 16-state lifecycle engine, and the orchestrator daemon are implemented and merged. The orchestrator runs as a background process (`ao start`), manages agent sessions end-to-end, and exposes a Unix domain socket IPC interface for all CLI commands.

## How it works

1. `ao start` launches the orchestrator daemon (IPC socket + 30s poll loop)
2. `ao spawn <github-issue-url>` creates an isolated git worktree, renders a prompt, and launches the configured AI agent in a tmux session
3. The orchestrator polls each session, drives the 16-state lifecycle (Spawning → Working → PrOpen → … → Merged/Done), and persists metadata to `~/.agent-orchestrator/`
4. `ao status` shows all active sessions; `ao session kill/cleanup` manage session lifecycle
5. `ao stop` gracefully shuts down the daemon

## CLI commands

| Command | Description |
|---------|-------------|
| `ao init` | Initialise `agent-orchestrator.yaml` in the current directory |
| `ao start` | Start the orchestrator daemon |
| `ao stop` | Stop the orchestrator daemon |
| `ao status` | Show all session statuses |
| `ao spawn <issue-url>` | Spawn an agent session for a GitHub issue |
| `ao batch-spawn <urls…>` | Spawn sessions for multiple issues |
| `ao send <session-id> <msg>` | Send a message to a running agent session |
| `ao session ls` | List sessions |
| `ao session kill <id>` | Kill a session |
| `ao session cleanup` | Clean up sessions in terminal tracker state |

## Packages

| Package | Description |
|---------|-------------|
| `packages/core` | Core library — orchestrator daemon, plugin traits, lifecycle engine, session store, prompt engine, IPC, config |
| `packages/cli` | `ao` CLI binary — all 10 MVP commands |

## Plugin slots

| Slot | Default | Alternatives |
|------|---------|--------------|
| Agent | `claude-code` | codex, aider, opencode, gemini, openclaw |
| Runtime | `tmux` | process, Docker (planned) |
| Workspace | `worktree` | clone |
| Tracker | `github` | linear, Jira (planned) |
| SCM | `github` | GitLab (planned) |
| Notifier | `desktop` | slack, composio, webhook |
| Terminal | `iterm2` | web (planned) |
| Lifecycle | Core (not pluggable) | — |

## Getting started

### Prerequisites

```bash
make setup   # verifies and installs: rustup, gh CLI, tmux, gemini, codex
```

### Build

```bash
cargo build --workspace
```

### Run

```bash
# Start the orchestrator daemon
ao start

# Spawn an agent session for a GitHub issue
ao spawn https://github.com/owner/repo/issues/42

# Check session status
ao status

# Stop the daemon
ao stop
```

### Test

```bash
cargo test --workspace          # all tests
cargo test -p conductor-core    # core only
cargo clippy --workspace -- -D warnings
```

## Development

See [CLAUDE.md](./CLAUDE.md) for the full development workflow including the per-PR review gate.

Design documents live in [docs/](./docs/):
- `docs/prds/` — product requirements
- `docs/adrs/` — architecture decision records (8 accepted)
- `docs/plans/` — design docs and implementation plans
