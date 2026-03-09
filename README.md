# Conductor

Scalable orchestration layer for AI coding agents. Conductor manages parallel, isolated agent sessions — handling workspace creation, lifecycle monitoring, multi-turn execution, priority-based dispatch, and cost tracking across agents like Claude Code, Aider, Codex, and others.

## Status

**Walking skeleton complete.** `ao spawn <github-issue-url>` is wired end-to-end: CLI → IPC → Orchestrator → Workspace → PromptEngine → Agent → Runtime → SessionStore. Full MVP (remaining commands, poll cycle, 16-state lifecycle) is in progress.

## How it works

1. `ao spawn <github-issue-url>` creates an isolated git worktree for the issue
2. A tmux session is launched running the configured AI agent (default: Claude Code) with a rendered prompt
3. The orchestrator monitors the session, tracks state, and persists metadata to `~/.agent-orchestrator/`
4. On completion the worktree can be cleaned up and the session archived

## Packages

| Package | Description |
|---------|-------------|
| `packages/core` | Core library — orchestrator, plugin traits, session store, prompt engine, utils |
| `packages/cli` | `ao` CLI binary — `ao spawn` and supporting commands |

## Plugin slots

| Slot | Default | Alternatives |
|------|---------|--------------|
| Agent | `claude-code` | codex, aider, opencode, gemini, openclaw |
| Runtime | `tmux` | process, Docker (planned) |
| Workspace | `worktree` | clone |
| Tracker | `github` | linear |

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
cargo run -p cli -- spawn https://github.com/owner/repo/issues/42
```

Or after installing:

```bash
ao spawn https://github.com/owner/repo/issues/42
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
