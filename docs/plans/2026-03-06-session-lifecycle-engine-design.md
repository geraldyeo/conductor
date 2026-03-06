# Session Lifecycle Engine — Design Document

**Date:** 2026-03-06
**Status:** Approved
**Scope:** ADR for the session lifecycle state machine (PRD Section 5, FR15)

## Problem

The orchestrator needs a core engine that tracks session status across 16 states, detects real-world changes (runtime liveness, PR state, CI results, tracker state), and transitions sessions accordingly. This engine is the heartbeat of the system — every other feature (CLI, dashboard, reactions) is an input to or output of it.

The PRD (Section 5) specifies the transition table with 30 precedence-ordered rows. The architectural question is: how should this be represented and evaluated in code?

## Decision: Graph-Driven State Machine

Represent the transition table as a **directed graph** (with cycles). Nodes are session statuses. Edges are guarded transitions with precedence. Evaluation walks outgoing edges from the current node.

### 1. Graph Structure

```
Node {
  status: SessionStatus              // e.g., "working", "pr_open"
  terminal: boolean                  // true for killed, terminated, done, cleanup, errored, merged
  edges: Edge[]                      // outgoing transitions, ordered by precedence (ascending)
}

Edge {
  to: SessionStatus                  // target status
  precedence: number                 // from PRD Section 5.3 (lower wins)
  guard: (ctx: PollContext) => boolean   // pure function, no side effects
}
```

**Wildcard transitions** (e.g., `any -> killed` on manual kill, `any -> cleanup` on tracker terminal state) are modeled as **global edges**. At graph construction time, these are appended to every non-terminal node's edge list at their declared precedence. This keeps per-node edge lists self-contained while avoiding 16 duplicate definitions.

**Evaluation** for a session:

1. Look up the node for the session's current status.
2. Walk its edges in precedence order.
3. First guard that returns `true` fires the transition.
4. No guard matches: status unchanged.

This is **O(out-degree)** per session per poll cycle, not O(total edges).

### 2. Poll Context (Input Gathering)

Each poll cycle gathers data into a PollContext — the single object passed to all guard functions.

```
PollContext {
  // Runtime
  runtimeAlive: boolean

  // Agent activity
  activityState: ActivityState        // active, ready, idle, waiting_input, blocked, exited

  // PR state (null if no PR detected yet)
  pr: {
    detected: boolean
    state: "open" | "merged" | "closed"
    ciStatus: "pending" | "green" | "failed"
    reviewDecision: "none" | "approved" | "changes_requested"
    mergeable: boolean
  } | null

  // Tracker
  trackerState: "active" | "terminal"

  // Budget
  budgetExceeded: boolean

  // Manual actions
  manualKill: boolean
}
```

**Gathering strategy:** inputs are collected sequentially per session in a fixed order — runtime liveness first (cheapest, short-circuits if dead), then activity state, then PR/CI/review (most expensive, involves API calls), then tracker state.

**Across sessions:** multiple sessions are gathered and evaluated concurrently (bounded by a concurrency limit) since they are independent.

Guard functions are pure — they read from the context, never fetch data. This keeps gathering and evaluation cleanly separated.

### 3. Graph Construction and Validation

The graph is built from a declarative definition — a flat list of edge declarations compiled into the adjacency list.

```
// One line per row in PRD Section 5.3
defineEdge("spawning",          "working",            1,  ctx => ctx.runtimeAlive && ctx.activityState === "active")
defineEdge("spawning",          "errored",            2,  ctx => !ctx.runtimeAlive)
defineEdge("working",           "pr_open",            3,  ctx => ctx.pr?.detected)
defineEdge("working",           "needs_input",        4,  ctx => ctx.activityState === "waiting_input")
defineEdge("working",           "stuck",              5,  ctx => ctx.activityState === "idle")
defineEdge("working",           "errored",            6,  ctx => ctx.activityState === "blocked")
defineEdge("working",           "killed",             7,  ctx => !ctx.runtimeAlive)
defineEdge("working",           "done",               8,  ctx => ctx.activityState === "exited" && ctx.trackerState === "terminal")
defineEdge("working",           "terminated",         9,  ctx => ctx.activityState === "exited")
defineEdge("pr_open",           "ci_failed",         10,  ctx => ctx.pr?.ciStatus === "failed")
defineEdge("pr_open",           "review_pending",    11,  ctx => ctx.pr?.ciStatus === "green")
defineEdge("pr_open",           "working",           12,  ctx => ctx.activityState === "active")
defineEdge("pr_open",           "killed",            13,  ctx => !ctx.runtimeAlive)
defineEdge("ci_failed",         "working",           14,  ctx => ctx.activityState === "active")
defineEdge("ci_failed",         "killed",            15,  ctx => !ctx.runtimeAlive)
defineEdge("review_pending",    "changes_requested", 16,  ctx => ctx.pr?.reviewDecision === "changes_requested")
defineEdge("review_pending",    "approved",          17,  ctx => ctx.pr?.reviewDecision === "approved")
defineEdge("review_pending",    "ci_failed",         18,  ctx => ctx.pr?.ciStatus === "failed")
defineEdge("changes_requested", "working",           19,  ctx => ctx.activityState === "active")
defineEdge("changes_requested", "killed",            20,  ctx => !ctx.runtimeAlive)
defineEdge("approved",          "mergeable",         21,  ctx => ctx.pr?.ciStatus === "green" && ctx.pr?.mergeable)
defineEdge("approved",          "ci_failed",         22,  ctx => ctx.pr?.ciStatus === "failed")
defineEdge("mergeable",         "merged",            23,  ctx => ctx.pr?.state === "merged")
defineEdge("needs_input",       "working",           24,  ctx => ctx.activityState === "active")
defineEdge("needs_input",       "killed",            25,  ctx => !ctx.runtimeAlive)
defineEdge("stuck",             "working",           26,  ctx => ctx.activityState === "active")
defineEdge("stuck",             "killed",            27,  ctx => !ctx.runtimeAlive)

// Global edges (appended to all non-terminal nodes)
defineGlobalEdge("killed",  28, ctx => ctx.manualKill)
defineGlobalEdge("cleanup", 29, ctx => ctx.trackerState === "terminal")
defineGlobalEdge("killed",  30, ctx => ctx.budgetExceeded)
```

**Load-time validation:**

| Check | What it catches |
|-------|----------------|
| No orphan nodes | Every declared status appears as either a `from` or `to` |
| Terminal nodes have no outgoing edges | Prevents transitions out of `merged`, `killed`, etc. |
| All non-terminal nodes have at least one outgoing edge | No dead-end states that trap sessions |
| No duplicate precedence per node | Ambiguous evaluation order |
| Reachability from `spawning` | Every status is reachable from the initial state |
| Reachability to a terminal | Every non-terminal status has a path to at least one terminal |

Validation runs at startup and again on hot-reload (reject invalid graph, keep last-known-good).

### 4. Poll Loop Architecture

```
PollLoop {
  interval: number                // configurable, default 30000ms
  running: boolean                // re-entrancy guard
  graph: StateGraph               // the compiled adjacency list

  tick():
    if running: skip              // previous cycle still in-flight
    running = true

    sessions = loadActiveSessions()          // non-terminal only

    results = await concurrentMap(sessions, session => {
      ctx = gather(session)                  // sequential I/O per session
      edge = graph.evaluate(session.status, ctx)  // pure, O(out-degree)
      if edge: return { session, newStatus: edge.to, ctx }
      return null
    }, { concurrency: 5 })                   // bounded parallelism

    for result in results.filter(Boolean):
      transition(result.session, result.newStatus, result.ctx)

    running = false
```

Three distinct phases per tick:

1. **Gather** — I/O-heavy, parallelized across sessions (bounded). Each session's inputs are collected sequentially (runtime, activity, PR/CI, tracker).
2. **Evaluate** — Pure, no I/O. Walks the graph. Returns the winning edge or null.
3. **Transition** — Side-effecting. Updates session metadata (atomic write), fires entry actions. Applied sequentially to avoid races on shared resources.

### 5. Transition Side Effects and Entry Actions

```
transition(session, newStatus, ctx):
  previousStatus = session.status

  // Atomic write: temp file + rename
  session.status = newStatus
  session.previousStatus = previousStatus
  session.lastTransitionAt = now()
  session.save()

  // Append to action journal
  journal.append(session.id, {
    action: "transition",
    from: previousStatus,
    to: newStatus,
    timestamp: now(),
    actor: "lifecycle_engine"
  })

  // Fire entry actions
  actions = entryActions[newStatus]
  for action in actions:
    executeAction(session, action, ctx)
```

Entry actions are a simple map from status to action list — not part of the graph. This keeps the graph purely about state transitions.

```
entryActions = {
  "needs_input":       [notify("urgent", "Agent needs input")],
  "stuck":             [notify("urgent", "Agent stuck"), startStallTimer()],
  "killed":            [destroyRuntime()],
  "cleanup":           [destroyRuntime(), destroyWorkspace()],
  "merged":            [notify("info", "PR merged"), archiveSession()],
  "ci_failed":         [queueReaction("ci-failed")],
  "changes_requested": [queueReaction("changes-requested")],
  "errored":           [notify("warning", "Agent errored")],
  "done":              [notify("info", "Session complete")],
}
```

**Crash recovery:** on restart, load all non-terminal sessions and run one immediate poll tick. The graph evaluates current reality and converges — no event log to replay.

### 6. MVP Scope Boundary

**In — MVP:**

| Capability | Rationale |
|-----------|-----------|
| Graph structure + validation | Foundation — catches bugs at startup |
| All 16 statuses and the full transition table | The transition table is the spec |
| Poll loop with re-entrancy guard | The heartbeat |
| Gather: runtime liveness + activity state | Minimum to detect working/stuck/killed |
| Gather: PR detection by branch name | Minimum to progress past `working` |
| Gather: CI status + review decision | Needed for the full PR lifecycle path |
| Atomic metadata writes | Crash safety |
| Entry actions: `destroyRuntime`, `destroyWorkspace` | Cleanup on terminal states |
| Crash recovery via re-poll | Free — just the poll loop |

**Deferred — post-MVP:**

| Capability | Why defer |
|-----------|-----------|
| Action journal + idempotency checks | Single-agent MVP has low collision risk |
| Entry actions: notifications | Needs notifier plugins wired up |
| Entry actions: `queueReaction` | Needs reaction engine (FR4) |
| Budget enforcement | Nice-to-have, not blocking |
| Stall timer with configurable threshold | Simple timeout initially |
| Per-state concurrency limits (FR5) | Single-agent MVP does not need scheduling |
| Hot-reload of the graph | Config hot-reload is a separate ADR |
| Wait-for-ready protocol | Needs reaction engine |

## Consequences

**Positive:**
- The graph mirrors the PRD transition table 1:1 — easy to audit and verify.
- Guard functions are pure and testable without mocks.
- Graph validation catches structural errors (unreachable states, missing edges) at startup.
- Scoped evaluation (O(out-degree)) is efficient even as the graph grows.
- Crash recovery is a natural consequence of the design — no separate recovery codepath.
- Incremental delivery: new features (budget enforcement, reactions) add edges and entry actions without changing the core structure.

**Negative:**
- Global edges are duplicated across nodes at construction time — minor memory overhead.
- Debugging a "no transition fired" case requires inspecting all guards for the current node — mitigated by logging which guards were evaluated.
- The gather phase is sequential per session — could become a bottleneck with many slow API calls. Acceptable for MVP; can be optimized to parallel gathering with dependency ordering later.
