# 🏗️ Architecture Decision Records (ADR)

## ADR-001: "Eight Slot" Plugin Architecture
- **Context:** We need to support rapidly evolving AI agents and varied infrastructure (GitHub vs Linear, tmux vs Docker).
- **Decision:** Adopt a strict "Slot" system where every core capability is a swappable plugin.
- **Rationale:** Prevents vendor lock-in and allows the community to contribute new runtimes (e.g., E2B) or trackers (e.g., Jira) without modifying the core orchestrator.

## ADR-002: Isolation via Git Worktrees
- **Context:** Traditional `git clone` is slow and storage-intensive for large repos. Branch switching in a single clone prevents parallelism.
- **Decision:** Use `git worktree` as the default workspace strategy.
- **Rationale:** Worktrees share the underlying `.git` object store but provide unique directories and branches. This allows instantaneous creation of isolated environments for parallel agent execution.

## ADR-003: Terminal Multiplexing with tmux
- **Context:** We need a way to run CLI-based agents (like Claude Code) that allows humans to "attach" and interact if the agent gets stuck.
- **Decision:** Use `tmux` as the primary local runtime.
- **Rationale:** `tmux` provides robust session management, persists through shell disconnects, and natively supports the "attach" workflow for human-in-the-loop debugging.

## ADR-004: Event-Driven Lifecycle Polling
- **Context:** Agents, CI systems, and PRs change state asynchronously.
- **Decision:** Implement a centralized `LifecycleManager` that performs periodic polling of runtimes and trackers.
- **Rationale:** While webhooks are "pushed," polling is more resilient to network blips and works across all platforms (local tmux sessions don't have webhooks). It acts as a reliable state machine.

## ADR-005: Rule-Based Automated Reactions
- **Context:** Not all events require human intervention (e.g., a flaky CI failure).
- **Decision:** Define a `reactions` configuration that maps event types (e.g., `ci.failing`) to actions (e.g., `send-to-agent`).
- **Rationale:** Decouples the "what happened" from the "what to do," making the orchestrator highly customizable for different team workflows.

## ADR-006: Local File-Based Persistence
- **Context:** We need to track session IDs, hashes, and statuses across restarts.
- **Decision:** Use a local data directory (`~/.agent-orchestrator`) storing JSON/YAML metadata.
- **Rationale:** Simplifies setup by avoiding the need for an external database (Postgres/Redis) while providing enough durability for a developer-centric tool.
