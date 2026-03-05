# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Conductor** (Agent Orchestrator) is a scalable orchestration layer for AI coding agents (Claude Code, Aider, Codex, Goose). It enables parallel, isolated agent sessions with automated lifecycle management — workspace creation, CI monitoring, review handling, and optional auto-merge.

## Status

This project is in the **design/documentation phase**. No source code exists yet. Key design documents live in `docs/`:

- `docs/PRD.md` — Functional and non-functional requirements
- `docs/ADR.md` — Architecture decision records

## Architecture: "Eight Slot" Plugin System

The core design is a modular plugin architecture with eight swappable slots:

| Slot | Purpose | Examples |
|------|---------|----------|
| **Runtime** | Execution environment | tmux, Docker, Kubernetes, E2B |
| **Agent** | AI coding logic | Claude Code, Aider |
| **Workspace** | Filesystem isolation | git worktree (default), git clone |
| **Tracker** | Issue source | GitHub Issues, Linear, Jira |
| **SCM** | Source control / PR/CI | GitHub, GitLab |
| **Notifier** | Alerts & communication | Slack, Discord, Desktop |
| **Terminal** | Interactive human-agent UI | — |
| **Lifecycle** | Global orchestration logic | — |

Each slot is a TypeScript interface that plugins implement.

## Key Design Decisions

- **Git worktrees** for workspace isolation (fast, shared `.git` object store)
- **tmux** as primary local runtime (supports human attach/detach)
- **Polling-based lifecycle** over webhooks (more resilient, works with local sessions)
- **Rule-based reactions** mapping events (`ci.failing`, `review.comment`) to actions (`send-to-agent`, `notify`)
- **Local file persistence** in `~/.agent-orchestrator` (JSON/YAML, no external DB)

## Implementation Notes

When building out this project:

- New plugins should implement the corresponding slot's TypeScript interface
- The `LifecycleManager` is the central polling coordinator for runtime and tracker state
- Session state must survive orchestrator restarts via the local persistence layer
- The CLI command is `ao` with subcommands: `spawn`, `status`, `send`, `kill`
