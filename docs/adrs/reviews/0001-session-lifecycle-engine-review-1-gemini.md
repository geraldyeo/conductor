# ADR-0001: Session Lifecycle Engine -- Review Round 1 (Gemini)

**Reviewer:** Gemini
**Date:** 2026-03-06
**ADR Status at review time:** Proposed
**Referenced documents:**
- `docs/adrs/0001-session-lifecycle-engine.md`
- `docs/plans/2026-03-06-session-lifecycle-engine-design.md`
- `docs/prds/0001-agent-orchestrator.md` (v1.3, Accepted)
- `AGENTS.md`

---

## Summary

ADR-0001 proposes a graph-driven state machine for the session lifecycle engine. The ADR is well-structured, clearly reasoned, and makes a defensible architectural choice. The graph-driven approach offers meaningful advantages over the alternatives considered, particularly in auditability, testability, and structural validation. However, there are gaps in PRD alignment, some unstated risks, and areas where the ADR could be more precise.

---

## Strengths

1. **Strong problem framing.** The Context section (lines 7-19) clearly articulates the forces driving the decision: auditability, testability, incremental delivery, simple crash recovery, and evaluation efficiency. These are well-chosen criteria for this domain.

2. **Fair option comparison.** All four options are presented with genuine pros and cons. The State Pattern critique (line 23, "16 classes of boilerplate... transition logic scattered across files") and the Reducer Pattern critique (line 24, "becomes a very large function") are accurate assessments. The chosen option's cons are not hidden.

3. **Excellent separation of concerns.** The six-component decomposition (Graph Structure, Poll Context, Graph Construction/Validation, Poll Loop, Transition Side Effects, MVP Scope) provides a clean mental model. Separating entry actions from the graph (line 38) is a particularly sound design choice.

4. **Pure guards and testability.** The commitment to pure guard functions over PollContext structs (line 32) enables straightforward unit testing without mocks. This is a meaningful architectural property.

5. **Crash recovery simplicity.** The observation that crash recovery is a natural consequence of the poll-based design (lines 51, "No event log to replay, no separate recovery codepath") is elegant and reduces implementation risk.

6. **PRD transition table fidelity.** The design doc's `defineEdge()` calls (design doc lines 88-119) map 1:1 to PRD Section 5.3 rows, which the ADR correctly highlights as a key benefit.

---

## Findings

### Critical

None.

### High

**H1. Missing transitions from the PRD transition table.**

The PRD Section 5.3 (PRD lines 434-436) specifies three global transitions:
- Row 28: `any -> killed` (manual kill)
- Row 29: `any -> cleanup` (tracker terminal state)
- Row 30: `any non-terminal -> killed` (budget exceeded)

Row 29 says `any -> cleanup`, but the design doc's `defineGlobalEdge` on line 119 guards on `ctx.trackerState === "terminal"`. The PRD row 29 says "any" (not "any non-terminal"), meaning a session already in `killed` or `errored` could transition to `cleanup` if the tracker reaches terminal state. However, the ADR states global edges are "appended to every non-terminal node" (line 30). This creates an inconsistency: if a session is already `killed`, a tracker terminal event would not trigger `cleanup` per the ADR, but the PRD says it should. The ADR should explicitly address whether terminal-to-terminal transitions are valid for the `cleanup` case and reconcile this with the PRD.

**H2. The `cleanup` status is listed as terminal but has no explicit transitions defined to it from non-global paths.**

The only path to `cleanup` is via the global edge (row 29). If a session is already in a terminal state like `errored` and the tracker subsequently reaches terminal, the session cannot transition to `cleanup` for workspace cleanup. This means sessions that error out before the tracker moves to terminal may leak workspaces. The ADR should clarify the cleanup strategy for sessions already in terminal states when the tracker reaches terminal.

**H3. Precedence semantics for global edges need clarification.**

Global edges have precedences 28-30, which means they are evaluated after all local edges. The ADR states they are "appended to every non-terminal node at construction time" (line 30, 34). But precedence 28 (manual kill) should arguably override most local transitions -- if a user issues `ao session kill` while the session is in `working` state, the manual kill should win regardless of whether the agent also just opened a PR. The current design handles this correctly by precedence ordering (28 > any local edge), but the ADR does not explicitly discuss whether there are scenarios where a global edge should preempt local edges. For example, budget exceeded (precedence 30) evaluates after `stuck -> working` (precedence 26), meaning a stuck agent that resumes activity would transition to `working` even if the budget is exceeded. This seems like a bug. The ADR should discuss whether budget enforcement and manual kill should have lower (higher-priority) precedence numbers.

### Medium

**M1. No discussion of the `terminated` vs `done` distinction.**

The ADR mentions both as terminal states but does not explain the semantic difference. The PRD (rows 8-9, lines 414-415) distinguishes them: `done` = agent exited cleanly AND issue is terminal; `terminated` = agent exited but issue is still active. This distinction matters for retry logic (a `terminated` session may trigger continuation retries per FR1). The ADR should note this.

**M2. Retry and continuation behavior not addressed.**

The PRD (FR1, FR4, Section 5.5) describes retry behavior, continuation retries, and exponential backoff as core lifecycle behaviors. The ADR's MVP scope (line 40) defers "stall timers" but does not mention retry/continuation at all. Since retries spawn new sessions rather than re-entering the state machine, this may be intentional, but the ADR should explicitly state that retry orchestration is outside the scope of the lifecycle engine and identify which component owns it.

**M3. The `readyThresholdMs` timer is not modeled.**

The PRD (line 204) defines `readyThresholdMs` (default 5 min) as the threshold before a `ready` activity state becomes `idle`. The design doc's guard for `working -> stuck` (line 92) checks `ctx.activityState === "idle"`, implying this threshold is applied during gathering. The ADR does not discuss where this timer lives or how the `ready -> idle` promotion is implemented. Since this affects guard evaluation, it should be mentioned.

**M4. No discussion of multi-turn session interaction.**

The PRD (FR1) describes multi-turn sessions where agents run up to `maxTurns` turns. The ADR does not address how turns interact with the lifecycle engine. Does the engine track turn count? Does it influence transitions? Is it part of PollContext? This is relevant to the "working" state -- an agent completing a turn and re-checking issue state may briefly appear as `ready` or `idle` between turns, which could trigger false `stuck` or `needs_input` transitions.

**M5. Considered Options section could mention event-sourcing explicitly as a rejected pattern.**

The Context section (line 16) mentions "no event sourcing" for crash recovery, but event sourcing is not listed as a Considered Option. Since event-sourced state machines are a common pattern in this domain, briefly noting why it was excluded (complexity, replay overhead) would strengthen the rationale.

### Low

**L1. Format compliance: missing date.**

The AGENTS.md ADR format (lines 53-73) does not mandate a date field, but it is conventional for ADRs and present in the design doc (design doc line 3). Consider adding a date to the ADR for traceability.

**L2. The ADR references the design doc (line 42) but does not specify the relationship.**

It would be helpful to state explicitly that the design doc contains the authoritative pseudocode and detailed rationale, and that the ADR is the decision summary. This helps readers understand the document hierarchy.

**L3. Terminal states list inconsistency.**

The ADR (line 9) lists terminal states as "killed, terminated, done, cleanup, errored" (5 states). The PRD (line 385) lists "killed, terminated, done, cleanup, errored, merged" (6 states). The design doc (line 23) also includes `merged`. The ADR's Context section omits `merged` from its terminal state enumeration. While the Decision section implicitly includes it (the graph has `mergeable -> merged`), the Context section should be consistent.

**L4. Minor: "30 precedence-ordered rows" count.**

The ADR (line 9) and design doc both state 30 rows. The design doc's edge definitions (lines 88-119) show exactly 27 `defineEdge` calls + 3 `defineGlobalEdge` calls = 30 total. The PRD table (lines 406-436) also has 30 rows. This checks out, but it is worth noting for future maintainers that adding edges requires updating this count in multiple places.

---

## Recommendations

1. **(H1, H2)** Clarify the `any -> cleanup` semantics. Either: (a) make it truly global (including terminal states), which requires an exception to the "terminal nodes have no outgoing edges" validation rule, or (b) explicitly state that cleanup for already-terminal sessions is handled outside the state machine (e.g., a separate garbage collection process). Document the chosen approach.

2. **(H3)** Reconsider whether `manualKill` and `budgetExceeded` global edges should have lower precedence numbers (e.g., 1-3) so they preempt local transitions. The current ordering means a budget-exceeded session could transition to a non-terminal state if a local edge fires first. At minimum, discuss this trade-off in the ADR.

3. **(M2)** Add a sentence to the MVP scope or Consequences section stating that retry/continuation orchestration is outside the lifecycle engine's scope and naming the responsible component (e.g., the scheduler or reaction engine).

4. **(M4)** Address multi-turn interaction, even briefly. State whether turn boundaries are visible to the lifecycle engine or handled entirely within the agent plugin's activity state reporting.

5. **(L3)** Add `merged` to the terminal states enumeration in the Context section.

---

## Verdict

**Disposition: Approve with revisions.** The core decision (graph-driven state machine) is sound and well-justified. The six-component decomposition is clean, and the MVP scope boundary is pragmatic. The high-severity findings (H1-H3) relate to edge-case semantics and precedence ordering rather than fundamental design flaws, but they should be addressed before the ADR moves to Accepted status, as they could lead to subtle bugs in implementation (workspace leaks from H2, budget bypass from H3).
