# PRD Review: Agent Orchestrator (Conductor) — Round 2
**Reviewer:** Gemini CLI
**Date:** Friday, 6 March 2026
**PRD Version:** 1.1 (Incorporating Round 1 feedback)

## 1. Executive Summary
Round 2 of the PRD represents a significant leap toward a production-ready system. The addition of the **Mutation Authority Model (FR17)** and the **State Transition Table (Section 5.3)** provides the deterministic control needed for a scalable multi-agent environment. The incorporation of **Budget Enforcement** and the **Action Journal (FR4/FR15)** directly addresses the primary risks identified in Round 1 (cost explosions and PR churn).

The architecture has successfully transitioned from a "reactive script" model to a "robust distributed coordinator" model.

---

## 2. Strengths of the Updated PRD

### A. Mutation Authority Model (FR17) — **CRITICAL STRENGTH**
The move from probabilistic prompt guidance to **mechanical tool-level enforcement** is a major win. By withholding lifecycle tools (merge, close, delete) from worker agents, the orchestrator eliminates "agent overreach" by design. This ensures that policy-heavy actions (like merging a PR) remain strictly under the control of the orchestrator's state machine.

### B. Precedence-Based State Transitions (Section 5.3)
The formal transition table is a vast improvement over the previous logic. Defining a single source of truth for valid states with explicit precedence (1-30) makes the system predictable and significantly easier to unit test. It also provides a clear framework for handling complex states like `ci_failed` vs. `review_pending`.

### C. Action Journal & Idempotency (FR15)
The introduction of a per-session append-only log with dedupe keys effectively solves the "PR churn" problem. This ensures that even if the polling cycle hits a rate limit or a blip, the orchestrator won't accidentally trigger duplicate merges or re-spawn loops.

---

## 3. Categorized Findings & Recommendations

### [High] Cooperation Protocol: Interrupt vs. Wait
The "Conflict resolution rule" in FR17 states the orchestrator "defers to a running agent," but the specific timing remains slightly underspecified.
- **Evidence:** Section 5.3 Transitions 10-18 cover external events (CI fails, review requested).
- **Risk:** If an agent is in turn 8 of 20 (Activity State: `active`), and a CI failure is detected (Session Status: `ci_failed`), does the orchestrator `ao send` a fix instruction *immediately*? This could disrupt the agent's current multi-turn reasoning loop.
- **Recommendation:** Refine the precedence logic to explicitly wait for `Activity State: ready` or `idle` before delivering "nudges" for external events, unless the event is a "Hard Cancellation" (issue closed).

### [Medium] Action Journal Error Tracking
The Action Journal (FR15) tracks *executed* actions but doesn't explicitly mention *failed* execution attempts.
- **Evidence:** FR4 mentions "reaction tracker maintains per-session attempt counts."
- **Risk:** If a merge fails due to a temporary GitHub outage, the journal might lack the context needed for the Reaction Engine to decide whether to retry or escalate.
- **Recommendation:** Ensure the Action Journal captures the *result* (Success/Failure/Error) of each action, providing a rich history for the Reaction Engine's escalation logic.

### [Medium] Cross-Session Awareness (Inter-Agent Conflict)
This is currently listed as a "Future enhancement" (FR5).
- **Risk:** In a "scalable" environment with 10+ concurrent agents, two agents fixing different bugs in the same module will frequently produce merge conflicts.
- **Recommendation:** While full conflict detection is complex, a "Low-Effort" mitigation would be to add a `modified_files` field to the session metadata. The **Orchestrator-Agent (FR13)** can then perform a simple overlap check to warn users or sequence sessions.

### [Low] Budget Enforcement Granularity
The budget is tracked per session (FR4).
- **Recommendation:** Consider adding a **Global Daily Budget** in the top-level config. This prevents a "batch spawn" of 50 agents from draining a monthly API credit limit in a single hour if the per-session limits are set too loosely.

---

## 4. Final Verdict
The PRD is **Accepted with minor refinements**. The structural integrity is high, and the mutation authority model is a benchmark for AI-agent orchestration. The project is ready to move from the **Design Phase** to **Finalizing ADR-0007 (Implementation Language)** and initial scaffolding.

**Action Item:** Update Section 5.4 (Status Determination) to include the "wait-for-ready" protocol for non-terminal nudges.
