# GEMINI.md

This file provides guidance to Gemini CLI when working in this repository.

## Project Overview

**Conductor** (Agent Orchestrator) is a scalable orchestration layer for AI coding agents (Claude Code, Aider, Codex, OpenCode, Gemini CLI, OpenClaw). It combines active lifecycle management (PR/CI/review monitoring) with a scheduler/reader model (agents self-manage tracker state). It enables parallel, isolated agent sessions with automated workspace creation, multi-turn execution, priority-based dispatch, and cost tracking.

Inspired by ComposioHQ's agent-orchestrator (plugin architecture, reaction engine) and OpenAI's Symphony (multi-turn sessions, concurrency limits, hot-reload, tracker-driven scheduling).

## Status

This project is in the **design/documentation phase**. No source code exists yet. Key design documents live in `docs/`:

- `docs/prds/` — Product requirements documents and reviews
- `docs/adrs/` — Architecture decision records (7 foundational ADRs)
- `docs/plans/` — Design documents and implementation plans

## Document Conventions

See `AGENTS.md` for shared conventions (PRD/ADR naming, review file naming, ADR format, commit style). This file covers Gemini-specific guidance only.

## Architecture

Monorepo with packages: `cli`, `core`, `dashboard`, `mobile`. Language and toolchain TBD (see ADRs).

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

## Gemini-Specific Guidance

### Tool Usage & Efficiency

- **Research First**: Use `grep_search` and `glob` to map the codebase (especially documentation currently).
- **Validate Assumptions**: Always use `read_file` to confirm the content of a file before making changes or referencing it in a review.
- **Surgical Edits**: Prefer `replace` over `write_file` for targeted changes to large files.
- **Background Processes**: If a command is long-running (like a dashboard server or watcher), use `is_background: true`.

### Commit Style

Strictly follow the `AGENTS.md` commit convention:
- Use Conventional Commits.
- Include the `Co-Authored-By: Gemini CLI <noreply@google.com>` trailer in every commit message.
- Use appropriate scopes: `prd`, `adr`, `plan`, `config`, etc.

### Review Workflow

- When reviewing docs, follow the `AGENTS.md` naming pattern: `docs/{folder}/{NNNN}-{title}-review-{round}-gemini.md`.
- Categorize by severity: Critical, High, Medium, Low.

### Coding Standards (Future)

- Once ADR-0007 (Implementation Language) is finalized, adhere strictly to the chosen language's idioms and safety rules.
- Maintain isolation by using `git worktree` for manual testing if required.
- Ensure all logic is consolidated into clean abstractions per ADR-0001 (Plugin Architecture).
