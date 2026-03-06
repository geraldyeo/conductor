# ADR-0001: Session Lifecycle Engine

## Status
Accepted

## Context
The orchestrator needs a core engine to track session status across 16 states, detect real-world changes (runtime liveness, PR state, CI results, tracker state), and transition sessions accordingly. This engine is the heartbeat -- every other feature (CLI, dashboard, reactions) is an input to or output of it.

The PRD (Section 5) specifies a transition table with 30 precedence-ordered rows covering the full session lifecycle: `spawning -> working -> pr_open -> review_pending -> approved -> mergeable -> merged`, with failure paths (`ci_failed`, `changes_requested`), human intervention states (`needs_input`, `stuck`), and terminal states (`killed`, `terminated`, `done`, `cleanup`, `errored`, `merged`).

Key forces:

- The transition table must be auditable against the PRD (1:1 mapping).
- Guard functions must be testable without mocks or I/O.
- The engine must support incremental delivery (MVP first, reactions/budget later).
- Crash recovery must be simple (no event sourcing).
- Evaluation must be efficient as session count grows.

The 6 activity states (active, ready, idle, waiting_input, blocked, exited) are inputs to the transition table, not statuses themselves. They are gathered as part of the poll context and consumed by guard functions.

## Considered Options
1. **Table-Driven State Machine** -- Flat array of `{ from, to, guard, precedence }` tuples. Evaluated top-to-bottom, first matching guard wins. Mirrors the PRD table directly. Pros: auditable, guards are pure functions, data-driven. Cons: evaluation is O(total edges) regardless of current state; no structural validation (orphan states, unreachable states go undetected).
2. **State Pattern (OOP)** -- Each of the 16 statuses is a class with an `evaluate(context)` method that returns the next status or null. Pros: encapsulates per-state logic, natural for entry/exit actions. Cons: 16 classes of boilerplate for what is essentially a lookup table; transition logic scattered across files making it hard to audit against the PRD; precedence ordering becomes implicit.
3. **Reducer Pattern (Functional)** -- A single pure function with pattern matching on current status and conditions on inputs. Like a Redux reducer. Pros: all logic in one place, highly testable, simple mental model. Cons: becomes a very large function (16 statuses x multiple transitions each); precedence is implicit in code order within each case; less composable for hot-reload or runtime overrides.
4. **Graph-Driven State Machine** -- Directed graph (with cycles) where nodes are statuses and edges are guarded transitions with precedence. Each node has an ordered list of outgoing edges. Evaluation walks only the outgoing edges of the current node. Pros: O(out-degree) evaluation, structural validation at load time (reachability, dead ends, orphan nodes), mirrors PRD 1:1 via declarative `defineEdge()` calls, natural for visualization. Cons: global edges (wildcard transitions like `any -> killed`) are duplicated across nodes at construction time; debugging "no transition fired" requires inspecting all guards for the current node.
5. **Event-Sourced State Machine** -- Persist every state-changing event to an append-only log; derive current state by replaying events. Pros: full audit trail, time-travel debugging, replay-based recovery. Cons: replay latency grows with event count; requires snapshot compaction; adds an event store dependency; recovery complexity is high for an MVP that can achieve convergence by re-polling live state.

## Decision
Option 4: Graph-Driven State Machine. The design has six components:

1. **Graph Structure** -- Nodes hold `{ status, terminal, edges[] }`. Edges hold `{ to, precedence, guard }`. Wildcard transitions (e.g., `any -> killed` on manual kill, `any -> cleanup` on tracker terminal) are modeled as global edges, appended to every non-terminal node at construction time. Evaluation: look up node for current status, walk edges in precedence order, first matching guard fires.

2. **Poll Context** -- A `PollContext` struct gathered fresh each tick: `{ runtimeAlive, activityState, pr: { detected, state, ciStatus, reviewDecision, mergeable } | null, trackerState, budgetExceeded, manualKill }`. Guards are pure functions over this context -- no I/O in evaluation. Gathering is sequential per session (runtime -> activity -> PR/CI -> tracker), concurrent across sessions (bounded parallelism).

3. **Graph Construction and Validation** -- Declarative `defineEdge(from, to, precedence, guard)` and `defineGlobalEdge(to, precedence, guard)` calls, one per PRD transition row. Compiled into an adjacency list. Load-time validation checks: no orphan nodes, terminal nodes have no outgoing edges, all non-terminal nodes have at least one outgoing edge, no duplicate precedence per node, reachability from `spawning` to every status, reachability from every non-terminal status to at least one terminal.

4. **Poll Loop** -- Fixed-interval tick (default 30s) with re-entrancy guard (skip if previous tick still running). Three phases per tick: (a) Gather -- I/O-heavy, concurrent across sessions, sequential within each session. (b) Evaluate -- pure graph walk, no I/O. (c) Transition -- side-effecting, sequential to avoid races. Updates session metadata via atomic write (temp file + rename).

5. **Transition Side Effects** -- Entry actions are a separate map from status to action list, not part of the graph. This keeps the graph purely about state navigation. Examples: `killed -> [destroyRuntime()]`, `cleanup -> [destroyRuntime(), destroyWorkspace()]`, `ci_failed -> [queueReaction("ci-failed")]`, `needs_input -> [notify("urgent", ...)]`. Action journal appended on each transition for auditability.

6. **MVP Scope** -- MVP includes: full graph with all 16 statuses and 30 transitions, poll loop with re-entrancy, gathering for runtime/activity/PR/CI/review/tracker, atomic metadata writes, entry actions for runtime/workspace cleanup, crash recovery via re-poll. Deferred: action journal idempotency, notification entry actions, reaction queuing, stall timers, per-state concurrency limits, hot-reload, wait-for-ready protocol. Note: budget enforcement and manual kill are in-scope for MVP (they are global edges with reserved precedence).

Reference `docs/plans/2026-03-06-session-lifecycle-engine-design.md` for full pseudocode and detailed rationale.

**Precedence bands.** Global edges that enforce hard constraints (manual kill, budget exceeded) use a reserved low-precedence band (0-2) so they always preempt local transitions. Local edges use the mid-range (3-27). Tracker-terminal cleanup uses the high range (28-29). Specifically: `manualKill` = 0, `budgetExceeded` = 1, local edges = 3-27, `trackerState === "terminal"` = 28. This prevents a scenario where a budget-exceeded session escapes to a non-terminal state via a lower-numbered local edge.

**Gather-to-transition staleness.** Between gathering a session's context and applying its transition, the real world may change (e.g., a runtime dies after being observed as alive). The system relies on convergence across poll cycles rather than single-cycle atomicity. A stale transition in one tick is corrected by the next tick's fresh gather. This is an acceptable trade-off for a 30-second polling interval.

**Terminal state self-sufficiency.** The `cleanup` global edge only appends to non-terminal nodes. Sessions already in a terminal state (e.g., `killed`, `errored`) handle their own resource cleanup via their entry actions. `killed` destroys the runtime; `merged` archives the session. A separate garbage collection sweep (outside the state machine) reclaims leaked workspaces from sessions whose entry actions failed. This avoids terminal-to-terminal transitions, which would complicate the graph's structural invariants.

**Timer-based triggers.** The lifecycle engine does not embed timers in the graph. Timer-driven behaviors (stall timeout, spawn timeout, `readyThresholdMs`) are handled in the gather phase: the gatherer promotes `ready` to `idle` based on elapsed time, and external timeout processes kill the runtime when stall thresholds are exceeded. Guards then detect the resulting state change (`activityState === "idle"`, `!runtimeAlive`). This keeps guards stateless and timers orthogonal to the graph.

**Retry and continuation scope.** Retry orchestration (exponential backoff, continuation retries, `maxRetriesPerIssue`) is outside the lifecycle engine's scope. The engine transitions sessions to terminal states; a separate scheduler component decides whether to spawn a new session for the same issue. This separation keeps the state machine focused on single-session lifecycle.

**Multi-turn sessions.** Turn boundaries within a multi-turn session are internal to the agent plugin's activity state reporting. The lifecycle engine sees only the resulting activity state (`active`, `ready`, `idle`). Between turns, the agent briefly enters `ready` state; the `readyThresholdMs` buffer (default 5 minutes) prevents false `stuck` transitions during inter-turn pauses.

## Consequences
Positive:

- The graph mirrors the PRD transition table 1:1 -- each `defineEdge()` call maps to one row in Section 5.3, making audits trivial.
- Guard functions are pure and testable without mocks -- construct a PollContext literal, assert the guard returns true/false.
- Graph validation catches structural errors (unreachable states, missing edges, dead ends) at startup, before any session runs.
- Scoped evaluation -- O(out-degree) per node -- is efficient even as the graph grows with new transitions.
- Crash recovery is a natural consequence: on restart, load non-terminal sessions and run one poll tick. The graph evaluates current reality and converges. No event log to replay, no separate recovery codepath.
- Incremental delivery: new features (budget enforcement, reactions, notifications) add edges and entry actions without changing the core graph structure.

Negative:

- Global edges are duplicated across all non-terminal nodes at construction time -- minor memory overhead (3 edges x 10 non-terminal nodes = 30 extra edge references).
- Debugging a "no transition fired" case requires inspecting all guards for the current node -- mitigated by logging which guards were evaluated and their results.
- The gather phase is sequential per session -- could become a bottleneck with many slow API calls. Acceptable for MVP; can be optimized to parallel gathering with dependency ordering later.
- The gather-to-transition staleness window means a session's context may be stale by the time its transition is applied. The system converges across poll cycles rather than guaranteeing single-cycle accuracy. At a 30-second interval, the worst-case staleness is one tick.
