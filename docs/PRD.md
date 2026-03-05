# 📋 Product Requirements Document (PRD): Agent Orchestrator

## 1. Executive Summary
**Vision:** To provide a scalable, parallel, and autonomous orchestration layer for AI coding agents (Claude Code, Aider, etc.), enabling them to work on complex engineering tasks with the same isolation and lifecycle management as human developers.

## 2. Problem Statement
AI coding agents are highly effective for individual tasks but face significant hurdles in a professional team environment:
- **Workspace Conflicts:** Running multiple agents in a single git clone causes branch and state collisions.
- **Monitoring Overhead:** Developers must manually watch CI logs and PR comments to "nudge" agents.
- **Context Loss:** Agents often lose track of the broader project conventions and issue-tracking metadata.
- **Lack of Parallelism:** Teams cannot easily spawn 5 agents to fix 5 different bugs simultaneously.

## 3. Target Audience
- **Software Engineering Teams:** Looking to automate maintenance, CI fixes, and minor feature requests.
- **DevOps/Platform Engineers:** Building automated "self-healing" CI pipelines.
- **AI Tooling Developers:** Seeking a robust runtime to host their agents.

## 4. Functional Requirements (FRs)

### FR1: Multi-Agent Support (Agnosticism)
- Must support multiple agent backends (Claude Code, Aider, Codex, Goose) through a standardized interface.
- Must allow custom agent configurations (models, permissions, rules) per project.

### FR2: Isolated Parallel Workspaces
- Must use `git worktree` (primary) or `git clone` to create isolated environments for every agent session.
- Must support automatic setup (dependency installation, symlinking `.env` files) for new workspaces.

### FR3: "Eight Slot" Plugin Architecture
The system must be modular across eight dimensions:
1. **Runtime:** Execution environment (tmux, Docker, Kubernetes, E2B).
2. **Agent:** The AI logic (Claude, Aider).
3. **Workspace:** Filesystem strategy (Worktree, Clone).
4. **Tracker:** Issue source (GitHub, Linear, Jira).
5. **SCM:** Source control platform (GitHub, GitLab) for PR/CI management.
6. **Notifier:** Communication (Slack, Discord, Desktop).
7. **Terminal:** Interactive UI for human-agent collaboration.
8. **Lifecycle:** Global orchestration logic.

### FR4: Autonomous "Reactions"
- **CI Failure Handling:** Automatically notify or re-trigger the agent when a PR’s CI fails.
- **Review Comment Handling:** Automatically spawn/re-awaken an agent to address reviewer comments.
- **Stuck Detection:** Detect when an agent is inactive for a defined period and alert a human.
- **Auto-Merge:** Optionally merge PRs once they are approved and all CI checks pass.

### FR5: Monitoring & Control
- **Web Dashboard:** A real-time visual overview of all active sessions, their status (e.g., `spawning`, `working`, `pr_open`, `ci_failed`), and logs.
- **CLI (`ao`):** Commands to `spawn`, `status`, `send` (instructions), and `kill` sessions.

## 5. Non-Functional Requirements (NFRs)
- **Extensibility:** New plugins should be addable by implementing a simple TypeScript interface.
- **Low Latency:** The orchestration loop (polling) should be configurable and efficient.
- **Persistence:** Session state must survive orchestrator restarts.
