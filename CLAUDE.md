# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Conductor** (Agent Orchestrator) is a scalable orchestration layer for AI coding agents (Claude Code, Aider, Codex, OpenCode, Gemini CLI, OpenClaw). It combines active lifecycle management (PR/CI/review monitoring) with a scheduler/reader model (agents self-manage tracker state). It enables parallel, isolated agent sessions with automated workspace creation, multi-turn execution, priority-based dispatch, and cost tracking.

Inspired by ComposioHQ's agent-orchestrator (plugin architecture, reaction engine) and OpenAI's Symphony (multi-turn sessions, concurrency limits, hot-reload, tracker-driven scheduling).

## Status

This project is in the **design/documentation phase**. No source code exists yet. Key design documents live in `docs/`:

- `docs/prds/` — Product requirements documents and reviews
- `docs/adrs/` — Architecture decision records (7 foundational ADRs)
- `docs/plans/` — Design documents and implementation plans

## Document Conventions

See `AGENTS.md` for shared conventions (PRD/ADR naming, review file naming, ADR format, commit style). This file covers Claude-specific guidance only.

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

## Tech Stack

Language and framework choices are TBD — see ADRs. Decided so far: tmux (runtime), `gh` CLI + GitHub GraphQL (SCM), GitHub Actions (CI), gitleaks (security).
