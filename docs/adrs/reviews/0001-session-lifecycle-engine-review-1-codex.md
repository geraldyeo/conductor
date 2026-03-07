# ADR-0001: Session Lifecycle Engine -- Review Round 1 (Codex)

**Reviewer:** Codex
**Date:** 2026-03-06
**ADR Status at Review:** Proposed
**Design Doc:** `docs/plans/2026-03-06-session-lifecycle-engine-design.md`
**PRD Reference:** `docs/prds/0001-agent-orchestrator.md` (version 1.3, Accepted)

---

## Summary

ADR-0001 proposes a graph-driven state machine to implement the session lifecycle engine described in PRD Section 5. The ADR is well-structured, follows the AGENTS.md format, and makes a sound architectural choice. The graph-driven approach is the strongest of the four options considered. However, there are gaps in edge-case handling, atomicity guarantees, and PRD alignment that should be addressed before acceptance.

---

## Strengths

1. **Strong option analysis.** Four distinct approaches are compared with concrete pros and cons. The table-driven vs. graph-driven distinction (O(total edges) vs. O(out-degree)) is well-articulated and the winning option is justified on structural validation and auditability grounds.

2. **1:1 PRD mapping.** The design doc's `defineEdge()` declarations (lines 88-119) map directly to the 30 rows in PRD Section 5.3 (lines 406-436). This makes auditing straightforward.

3. **Clean separation of concerns.** The gather-evaluate-transition pipeline and the decision to keep entry actions outside the graph are sound architectural choices that improve testability.

4. **Pure guard functions.** Making guards pure functions over a `PollContext` struct is excellent for testability and eliminates a large class of flaky-test problems.

5. **Graph validation at startup.** The six validation checks (orphan nodes, terminal edges, reachability, duplicate precedence) catch structural bugs early. This is a meaningful safety net.

6. **Crash recovery simplicity.** Re-polling on restart instead of replaying an event log is pragmatic and avoids the complexity of event sourcing for an MVP.

---

## Findings

### Critical

None.

### High

**H1. Race condition in the gather-evaluate-transition pipeline.**
The design doc (lines 148-159) shows that gather and evaluate run concurrently across sessions, but transitions are applied sequentially afterward. Between the time a session's context is gathered and the time its transition is applied, the real world may have changed. For example:

- Session A gathers context showing `runtimeAlive=true`.
- By the time the transition phase runs, the runtime has died.
- The engine transitions to a state that assumes a live runtime.

This is acknowledged implicitly by the 30-second polling interval (stale data is inherent), but the ADR should explicitly state the staleness window as a known trade-off and confirm that the next poll cycle will self-correct. Without this, readers may assume stronger consistency guarantees than exist.

**Recommendation:** Add a paragraph to the Consequences section acknowledging the gather-to-transition staleness window and stating that the system relies on convergence across poll cycles rather than single-cycle atomicity.

**H2. Global edge precedence interaction with per-node edges is underspecified.**
The design doc (lines 116-119) declares global edges at precedences 28-30. These are appended to every non-terminal node. But the ADR and design doc do not address what happens if a node has local edges at or near these precedence numbers. The validation check for "no duplicate precedence per node" (line 129) would catch exact collisions, but the semantic interaction is not discussed.

More importantly, consider `mergeable` (line 110): it has one local edge at precedence 23. The global edges at 28-30 mean that `manualKill`, `trackerState=terminal`, and `budgetExceeded` can fire from `mergeable`. But what about `mergeable -> killed` when `!runtimeAlive`? There is no such edge. If the runtime dies while in `mergeable`, no transition fires until the next cycle when the PR might have been merged. This is a potential gap.

**Recommendation:** (a) Explicitly document the precedence band convention (e.g., 1-27 for local edges, 28-30 for global edges). (b) Audit whether `mergeable` needs a `!runtimeAlive -> killed` edge. The PRD does not list one, but it may be an oversight in both documents.

**H3. The `cleanup` terminal status has no entry path from most states.**
PRD line 435 says `any -> cleanup` when the issue reaches a terminal tracker state. The design doc models this as a global edge (line 118). However, `cleanup` is listed as a terminal status (design doc line 23, PRD line 385). This means a session in `merged` (also terminal) would not get the `cleanup` global edge because global edges are only appended to non-terminal nodes. That is correct -- but it means the entry actions for `merged` must independently handle cleanup (or the `merged` entry action `archiveSession()` must subsume cleanup duties). The ADR does not clarify this relationship.

**Recommendation:** Clarify in the ADR that terminal-to-terminal transitions are intentionally excluded and that each terminal state's entry actions are self-sufficient for their cleanup responsibilities.

### Medium

**M1. Missing edges for `review_pending` and `approved` when runtime dies.**
PRD Section 5.3 does not list `review_pending -> killed` or `approved -> killed` when `!runtimeAlive`. The design doc mirrors this omission. However, it is plausible that a runtime dies while waiting for review or after approval. The global edge `any -> killed` on `manualKill` covers manual kills, and `any -> cleanup` covers tracker-terminal, but there is no explicit path for `!runtimeAlive` from these states.

Counter-argument: in `review_pending` and `approved`, the agent may not need to be alive (the orchestrator is waiting for external events). If the runtime dies, it does not block progress. But when the session eventually needs to transition to `working` (e.g., on `changes_requested`), the dead runtime becomes a problem.

**Recommendation:** Either add `!runtimeAlive -> killed` edges for `review_pending` and `approved`, or document why the engine intentionally tolerates a dead runtime in these states and how recovery works.

**M2. The ADR does not discuss timer-based transitions.**
PRD Section 5.3 row 5 says `working -> stuck` triggers on "Activity state = `idle` beyond threshold." The design doc guard (line 92) checks `ctx.activityState === "idle"` but does not encode the threshold. The threshold logic is presumably in the gather phase (activity state transitions from `ready` to `idle` after `readyThresholdMs`), but this is not stated.

Similarly, PRD row 27 says `stuck -> killed` triggers on "Stall timeout exceeded, agent killed." The design doc guard (line 114) checks `ctx => !ctx.runtimeAlive`, which is not the same thing -- a stall timer would kill the runtime first, then the guard detects the dead runtime. This two-step mechanism is not documented.

**Recommendation:** Document how timer-based triggers (stall threshold, spawn timeout) interact with the guard system. Are they external processes that modify reality (e.g., killing the runtime), which the guards then detect? Or should `PollContext` include timer state?

**M3. `PollContext` is missing fields for some PRD transitions.**
The `PollContext` struct (design doc lines 48-73) does not include:
- A field for spawn timeout (needed for `spawning -> errored` at precedence 2; the guard checks `!ctx.runtimeAlive` but the PRD says "Agent failed to start within timeout").
- A field for stall duration (needed for `stuck -> killed`).
- A field for PR `closed` state (the PR state enum includes `"open" | "merged" | "closed"` but no guard checks for `closed`).

**Recommendation:** Either extend `PollContext` with these fields or document that certain triggers are handled outside the guard system (e.g., timers kill the process, then `!runtimeAlive` fires).

**M4. No discussion of concurrent transitions to the same target.**
If two sessions both transition in the same tick and both trigger entry actions that call `destroyRuntime()` or interact with the same tracker issue, there could be contention. The sequential transition phase (design doc line 156) prevents intra-tick races, but entry actions that make external API calls could still conflict across ticks.

**Recommendation:** Acknowledge this in the Consequences section and note that entry action idempotency (deferred post-MVP) mitigates this.

### Low

**L1. Terminal status list inconsistency.**
The ADR (line 9) lists terminal states as `killed`, `terminated`, `done`, `cleanup`, `errored`. The design doc (line 23) lists `killed`, `terminated`, `done`, `cleanup`, `errored`, `merged`. The PRD (line 385) lists `killed`, `terminated`, `done`, `cleanup`, `errored`, `merged`. The ADR omits `merged` from its terminal list in the Context section.

**File/Line:** `docs/adrs/0001-session-lifecycle-engine.md`, line 9.

**Recommendation:** Add `merged` to the terminal state list in the ADR Context section.

**L2. The `cleanup` status semantics are underdefined.**
`cleanup` is a terminal state, but it also has entry actions (`destroyRuntime`, `destroyWorkspace`). What if those actions fail? The ADR and design doc do not discuss failure handling for entry actions on terminal states. A session stuck in `cleanup` with a failed workspace deletion would be in a terminal state with leaked resources.

**Recommendation:** Note that entry action failures on terminal states should be logged and retried (or handled by a separate garbage collection mechanism), but do not block the terminal status.

**L3. Format nit: ADR uses em-dashes in Considered Options.**
AGENTS.md (line 66) specifies `--` (double hyphen) as the separator in Considered Options. The ADR uses `--` correctly. No issue here -- this is confirmed as compliant.

**L4. The Considered Options section could mention event-sourced approaches.**
Event sourcing is briefly dismissed in the Context section ("no event sourcing") but is not listed as a considered option. Given that event sourcing is a common pattern for state machines, its explicit rejection as an option would strengthen the analysis.

**Recommendation:** Consider adding a brief fifth option for event sourcing with rationale for rejection, or at minimum reference it in the Context section's "Crash recovery must be simple" force.

---

## PRD Alignment Checklist

| PRD Requirement | ADR Coverage | Notes |
|----------------|-------------|-------|
| 16 session statuses (5.1) | Covered | All 16 present in graph |
| 6 activity states (5.2) | Covered | Modeled as `PollContext.activityState` |
| 30 transition rows (5.3) | Covered | 1:1 mapping via `defineEdge()` |
| Precedence ordering (5.3) | Covered | Edges sorted by precedence |
| Gather-then-evaluate (5.4) | Covered | Three-phase pipeline |
| Terminal statuses (5.1) | Partially | `merged` omitted from ADR terminal list (L1) |
| Retry behavior (5.5) | Deferred | Exponential backoff listed as post-MVP |
| Stall detection (5.5) | Partially | Timer mechanism not documented (M2) |
| Budget enforcement (FR4) | Deferred | Global edge exists but enforcement deferred |
| Per-state concurrency (FR5) | Deferred | Explicitly listed as post-MVP |
| Action journal idempotency (FR15) | Deferred | Explicitly listed as post-MVP |

---

## Verdict

**Recommendation: Accept with revisions.**

The core architectural decision (graph-driven state machine) is sound and well-justified. The ADR should be revised to address the High findings (H1-H3) before moving to Accepted status. The Medium findings (M1-M3) should be addressed or explicitly deferred with rationale. The remaining items are minor.

The MVP scope boundary is realistic for a single-agent local deployment. The deferred items are correctly identified as non-blocking for initial functionality.
