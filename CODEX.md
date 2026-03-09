# CODEX.md

This file provides guidance to Codex when working in this repository.

## Project Overview

**Conductor** (Agent Orchestrator) is a scalable orchestration layer for AI coding agents (Claude Code, Aider, Codex, OpenCode, Gemini CLI, OpenClaw). It combines active lifecycle management (PR/CI/review monitoring) with a scheduler/reader model (agents self-manage tracker state). It enables parallel, isolated agent sessions with automated workspace creation, multi-turn execution, priority-based dispatch, and cost tracking.

Inspired by ComposioHQ's agent-orchestrator (plugin architecture, reaction engine) and OpenAI's Symphony (multi-turn sessions, concurrency limits, hot-reload, tracker-driven scheduling).

## Status

**Walking skeleton complete.** The design/documentation phase is done (8 ADRs accepted). The project is in the **implementation phase** — `ao spawn <github-issue-url>` is wired end-to-end. Full MVP (remaining commands, poll cycle, 16-state lifecycle) is in progress.

Key design documents live in `docs/`:

- `docs/prds/` -- Product requirements documents and reviews
- `docs/adrs/` -- Architecture decision records (8 ADRs accepted)
- `docs/plans/` -- Design documents and implementation plans

## Document Conventions

See `AGENTS.md` for shared conventions (PRD/ADR naming, review file naming, ADR format, commit style). This file covers Codex-specific guidance only.

## Architecture

Monorepo with packages: `cli`, `core`, `dashboard`, `mobile`. Language: **Rust** (ADR-0002). Single Cargo workspace covering `packages/cli` and `packages/core`.

### Eight Slot Plugin System

Each plugin exports a manifest (name, slot, description, version) and a factory function.

| Slot | Implementations | Planned |
|------|----------------|---------|
| **Runtime** | tmux (default), process | Docker, K8s, E2B |
| **Agent** | claude-code (default), codex, aider, opencode, gemini, openclaw | Goose |
| **Workspace** | worktree (default), clone | -- |
| **Tracker** | github (default), linear | Jira |
| **SCM** | github | GitLab |
| **Notifier** | desktop, slack, composio, webhook | -- |
| **Terminal** | iterm2, web | -- |
| **Lifecycle** | Core (not pluggable) | -- |

### Key Patterns

- **Multi-turn sessions**: agents run up to N turns per session, re-checking issue state between turns
- **Prompt delivery modes**: `"inline"` (CLI arg) vs `"post-launch"` (runtime message) vs `"protocol"` (ACP/JSON-RPC)
- **5-layer prompt system**: base prompt + config context + skills directory + user rules + template rendering
- **Dynamic tools**: orchestrator advertises tools (e.g., `tracker_graphql`) that agents can call
- **Mutation authority model**: agents own work-level mutations (commit, PR, comments); orchestrator owns lifecycle mutations (merge, close, labels, spawn). Enforced mechanically via tool withholding.
- **Orchestrator-as-session**: `ao start` creates a special `-orchestrator` AI agent that coordinates workers
- **Reaction engine**: events -> actions (`send-to-agent`, `notify`, `auto-merge`) with retry, backoff, and escalation
- **Scheduling**: priority-based dispatch with per-state concurrency limits and blocker awareness
- **16 session statuses** with 6 activity states (see PRD Section 5)
- **Split ownership**: deterministic via tool-level enforcement, not probabilistic prompt rules
- **Token tracking**: per-session and aggregate input/output counters for cost visibility
- **Hot-reload config**: changes applied without restart, invalid changes keep last-known-good
- **4 workspace hooks**: afterCreate, beforeRun, afterRun, beforeRemove

## Codex-Specific Guidance

### Workflow

- Prefer quick repository discovery with `rg --files` and targeted text search with `rg`.
- Read `AGENTS.md` before writing PRD/ADR reviews to ensure filename and format compliance.
- For doc updates, keep edits minimal and deterministic; avoid broad rewrites unless requested.
- Verify line-level claims against files before finalizing a review.

### Review Output

- Save reviews in the correct folder and naming pattern:
  - PRD: `docs/prds/{NNNN}-{title}-review-{round}-codex.md`
  - ADR: `docs/adrs/{NNNN}-{title}-review-{round}-codex.md`
- Prioritize findings by severity: Critical, High, Medium, Low.
- Include concrete evidence references (`path:line`).
- Separate strengths from gaps and end with actionable recommendations.

### Commit Style

Strictly follow `AGENTS.md` commit conventions:
- Use Conventional Commits with appropriate scope (`prd`, `adr`, `plan`, etc.).
- Keep subject imperative, lowercase, and <= 50 characters.
- Include trailer: `Co-Authored-By: Codex <noreply@openai.com>`.

### Coding Standards

- Language: **Rust** (ADR-0002). Use `tokio` for async, `clap` for CLI, `serde`/`serde_yml` for config, `tera` for templates, `thiserror` for errors.
- Follow Rust idioms: exhaustive matching, explicit error propagation (`?`), `async-trait` for object-safe async traits.
- Build/test: `cargo build --workspace`, `cargo nextest run`, `cargo clippy --workspace -- -D warnings`.
- Preserve plugin-boundary abstractions from ADR-0004 (Plugin System).
- Maintain workspace isolation assumptions (git worktrees, symlink escape prevention) from ADR-0005.
