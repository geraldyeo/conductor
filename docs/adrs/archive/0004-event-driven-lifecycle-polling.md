# ADR-0004: Event-Driven Lifecycle Polling

## Status

Draft

## Context

The orchestrator must track the state of multiple concurrent agent sessions. State changes originate from diverse asynchronous sources: agent activity (detected via terminal output or JSONL logs), CI systems (GitHub Actions API), pull request reviews (GitHub GraphQL API), and runtime health (tmux session liveness).

The coordination mechanism must handle a complex state machine — 16 session statuses (spawning, working, pr_open, review_pending, approved, mergeable, merged, ci_failed, changes_requested, needs_input, stuck, errored, killed, done, terminated, cleanup) and 6 activity states (active, ready, idle, waiting_input, blocked, exited). It must detect transitions reliably and trigger the appropriate reactions.

## Considered Options

1. **Webhook-driven (push)** — Register webhooks for GitHub events (CI completion, PR reviews, comments). Agent state communicated via callbacks. Offers the lowest latency for external events. However, local tmux sessions have no webhook surface, requiring a publicly reachable endpoint (or a tunnel like ngrok) for GitHub to deliver events. Fragile under network issues. Each event source needs its own webhook handler and registration flow.

2. **Centralized polling loop** — A single LifecycleManager periodically checks all sessions: is the runtime alive? What is the agent's activity state? What is the PR/CI status? What is the review decision? Runs at a configurable interval (default 30 seconds). Re-entrancy guarded — if the previous poll is still running, the next cycle is skipped. Sessions are checked concurrently within each poll cycle.

3. **Hybrid (webhooks + polling fallback)** — Use webhooks where available (GitHub) for low-latency external event detection, and poll for local state (tmux liveness, agent activity). The most responsive approach but also the most complex — it requires maintaining two code paths for every external event source, with polling as the degradation path when webhooks fail.

## Decision

Option 2 — Centralized polling loop.

Polling works uniformly across all state sources, both local (tmux, agent output) and remote (GitHub API). It requires no public endpoint, no webhook registration, and no fallback logic. The 30-second default interval is acceptable for CI and PR state changes, which are not latency-sensitive in this context.

The architecture is designed to evolve toward Option 3 (hybrid) if latency requirements tighten. Adding webhook support later means polling becomes the fallback — no breaking changes required.

## Consequences

**Positive:**
- A single coordination path for all state sources simplifies the architecture and makes it testable.
- Resilient to network blips — a failed API call is retried on the next poll cycle automatically.
- Works natively with local tmux sessions, which have no webhook surface.
- No webhook infrastructure, tunnel, or public endpoint needed.
- Status determination logic is centralized in one place, making the state machine easy to reason about.

**Negative:**
- Up to 30-second delay in detecting external events (CI completion, review comments). Configurable but inherently bounded by the poll interval.
- GitHub API rate limits become a concern as the number of active sessions grows, since each session may trigger multiple API calls per poll cycle.
- The re-entrancy guard means a slow poll (e.g., due to API timeouts) delays the start of the next cycle.
