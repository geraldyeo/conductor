# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Conductor** (Agent Orchestrator) is a scalable orchestration layer for AI coding agents (Claude Code, Aider, Codex, OpenCode, Gemini CLI, OpenClaw). It combines active lifecycle management (PR/CI/review monitoring) with a scheduler/reader model (agents self-manage tracker state). It enables parallel, isolated agent sessions with automated workspace creation, multi-turn execution, priority-based dispatch, and cost tracking.

Inspired by ComposioHQ's agent-orchestrator (plugin architecture, reaction engine) and OpenAI's Symphony (multi-turn sessions, concurrency limits, hot-reload, tracker-driven scheduling).

## Status

Design phase complete (8 ADRs accepted). **Walking skeleton merged** — `ao spawn <github-issue-url>` is wired end-to-end across all architectural layers. Full MVP (remaining 9 commands, poll cycle, 16-state lifecycle) is in progress. See [Development Workflows](#development-workflows) below for how to contribute code. Key design documents live in `docs/`:

- `docs/prds/` — Product requirements documents and reviews
- `docs/adrs/` — Architecture decision records (8 ADRs accepted)
- `docs/plans/` — Design documents and implementation plans

## Document Conventions

See `AGENTS.md` for shared conventions (PRD/ADR naming, review file naming, ADR format, commit style). This file covers Claude-specific guidance only.

## Architecture

Monorepo with packages: `cli`, `core`, `dashboard`, `mobile`. Language: **Rust** (ADR-0002). Active packages: `packages/cli` (binary `ao`) and `packages/core` (library `conductor-core`).

### Eight Slot Plugin System

Each plugin exports a manifest (name, slot, description, version) and a factory function.

| Slot | Implementations | Planned |
|------|----------------|---------|
| **Runtime** | tmux (default), process | Docker, K8s, E2B |
| **Agent** | claude-code (default), codex, aider, opencode, gemini, openclaw | Goose |
| **Workspace** | worktree (default), clone | — |
| **Tracker** | github (default), linear | Jira |
| **SCM** | github | GitLab |
| **Notifier** | desktop, slack, composio, webhook | — |
| **Terminal** | iterm2, web | — |
| **Lifecycle** | Core (not pluggable) | — |

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

## Key Design Decisions

- **Git worktrees** for workspace isolation (fast, shared `.git` object store)
- **tmux** as primary local runtime (supports human attach/detach)
- **Polling-based lifecycle** (30s default) over webhooks (more resilient, works locally)
- **Rule-based reactions** mapping events to actions with retry/escalation/backoff
- **Flat file persistence** in `~/.agent-orchestrator/` (atomic writes, archive on delete)
- **Validated YAML config** with walk-up-tree discovery and hot-reload
- **Tracker-driven recovery** on restart (re-poll for active issues)

## Review Workflows

This project uses multi-model review via Claude Code slash commands that dispatch Gemini CLI and Codex CLI as independent reviewers.

- **`/adr-review <path>`** — Review ADRs and design docs. Dispatches Gemini + Codex in parallel, classifies findings as High/Medium/Low, synthesizes into a unified report. Use for all ADR acceptance reviews.
- **`/code-review-multi <file-or-'diff'>`** — Review source code or git diffs. Dispatches Gemini + Codex in parallel, classifies findings as Critical/Warning/Info, synthesizes with Claude's own review. Use `diff` as the argument to review uncommitted changes.

Both commands are defined in `~/.claude/commands/` and require `gemini` and `codex` CLIs to be installed.

**Gemini Code Assist** runs automatically on PRs. To trigger a manual re-review after pushing fixes, post a comment containing `/gemini review`. Old comment threads persist across review rounds — filter by comment ID to identify new findings. Address Critical/High findings before merging; reply with rationale to Mediums that reflect intentional design decisions.

## Tech Stack

Language: **Rust** (ADR-0002). Runtime: tmux. SCM: `gh` CLI + GitHub GraphQL. CI: GitHub Actions. Security scanning: gitleaks. Key crates: `tokio`, `clap`, `serde`/`serde_json`, `tera`, `thiserror`, `async-trait`, `strum`, `dirs`.

## Development Workflows

### Environment Setup

Run `make setup` to verify and install prerequisites. Requires:

- `rustup` (stable toolchain) — <https://rustup.rs>
- `gh` CLI — GitHub operations
- `tmux` — agent runtime
- `gemini` CLI — multi-model review
- `codex` CLI — multi-model review

Cargo tools (`cargo-nextest`) are installed by `make setup`. Dashboard and mobile toolchains are managed separately (TBD).

### Build & Test

Single Cargo workspace at repo root covers `packages/cli` and `packages/core`.

| Command | Purpose |
|---|---|
| `cargo build --workspace` | Build all packages |
| `cargo test --workspace` | Run all tests |
| `cargo test -p conductor-core` | Run tests for core package |
| `cargo clippy --workspace -- -D warnings` | Lint (warnings are errors) |
| `cargo fmt` | Format all code |
| `cargo fmt --check` | Check formatting (CI) |

Dashboard and mobile have their own toolchains (TBD) and are not part of the Cargo workspace.

### Feature Development Flow

Work is broken into independent tasks, each executed by a subagent in an isolated worktree. Use `superpowers:dispatching-parallel-agents` to parallelise independent tasks.

**Per-task flow:**

1. `EnterWorktree` — create an isolated workspace on a feature branch (`feat/<scope>/<short-description>`, e.g. `feat/core/session-store`)
2. Implement the task
3. Run the [Review Loop](#review-loop) below
4. Open a PR when review passes — one PR per worktree/task

**Humans review and merge PRs. Subagents do not merge.**

#### Conflict Resolution

Assign non-overlapping file/module ownership per task. If two tasks touch the same file, sequence them rather than parallelise.

When conflicts arise after another PR merges:

1. Rebase the worktree branch onto updated `main`
2. Re-run `/code-review-multi diff` on the rebase result
3. Update the PR

PRs with no conflicts merge first. For features with many parallel PRs, a coordinator subagent can manage merge ordering (applying the orchestrator-as-session pattern from ADR-0007 to development).

### Review Loop

Run within each worktree before opening a PR:

1. Run `/code-review-multi diff` — dispatches Gemini + Codex in parallel, classifies findings as Critical / Warning / Info
2. **Critical:** fix and re-run. Hard gate — no PR opens with an unresolved Critical.
3. **Warning:** fix and re-run. After 2 rounds, document remaining Warnings in the PR description with rationale. Humans decide in review.
4. **Info:** advisory only, no action required.
5. Open PR when no Critical findings remain.

**Every PR description must include:**

- What was implemented
- Any unresolved Warnings with rationale
- Number of review rounds completed
