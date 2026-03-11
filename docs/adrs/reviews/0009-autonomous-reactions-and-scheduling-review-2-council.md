# Council Review — ADR-0009: Autonomous Reactions and Scheduling

**Review round:** 2
**Date:** 2026-03-11
**Reviewers:** CC (Claude Code) · Gemini · Codex — excluded (stdin delivery failure, same as round 1)

---

## Council Verdict

The revised ADR-0009 successfully addresses all three confirmed high/medium findings from round 1: the crash-recovery argument is now correct (`reconcile_pending_reactions()`), `rework-requested` is demoted to a post-MVP stub, scheduler O-complexity is corrected with `schedulerMaxCandidatesPerTick`, and the dedupe key now includes `trigger_version`. The document is materially stronger.

However, two new confirmed findings emerged. **CF-1 (High)** — a stale-pending-queue race condition that both CC and Gemini independently identified — represents a correctness gap: the wait-for-ready protocol queues reactions while a session is `active`, but provides no mechanism to invalidate them if the underlying condition resolves before delivery. **CF-2 (Medium)** — notification spam for state-based (non-versioned) triggers — is flagged by both reviewers: reactions for `stuck`, `needs_input`, and `agent-exited` will re-notify every 60 seconds as long as the session remains in that state.

Gemini also raises two additional medium findings (all-complete triggering on empty project, trigger_version not mandated from plugins) that stand as individual concerns. **Recommendation: Address CF-1 and CF-2 before accepting; the remaining findings are lower priority.**

---

## Confirmed Findings *(CC + Gemini)*

### CF-1 · High · Stale pending queue — reactions delivered after condition resolves
**Flagged by:** CC (HIGH) · Gemini (HIGH-1)
**Section:** Component 1 — Reaction Engine, wait-for-ready delivery path
**Issue:** The wait-for-ready protocol queues non-terminal reactions while a session is `active` and delivers them on the next `idle`/`ready` tick. But there is no mechanism to invalidate or filter a queued reaction if the triggering condition resolves before delivery. Example: a `ci-failed` reaction is enqueued while the agent is mid-turn. While still active, the CI re-runs and passes, transitioning the session to `mergeable`. When the agent becomes idle, the stale `ci-failed` reaction pops from the queue and delivers a "your CI is failing" nudge to an agent whose CI is now green. This could send the agent on a spurious fix loop.
**Fix:** During the delivery gate check (step 1 of the delivery path), verify that the session's current state still satisfies the reaction's trigger condition before delivering. If the session's state no longer matches (e.g., session is `mergeable` but the pending reaction is `ci-failed`), discard the queued reaction rather than delivering it. This requires each `PendingReaction` to carry its `trigger_state: SessionStatus` and the delivery path to compare against the current `PollContext.session_status`.

### CF-2 · Medium · Notify spam for persistent non-versioned states
**Flagged by:** CC (MEDIUM) · Gemini (MEDIUM-2)
**Section:** Component 2 — Idempotency via Action Journal; Component 1 reaction table
**Issue:** Reactions for `agent-stuck`, `agent-needs-input`, and `agent-exited` have no `trigger_version` (no CI run ID, commit SHA, or review thread ID to anchor the dedupe key). With a 1-minute deduplication window, the reaction engine will send a new notification every 60 seconds for as long as the session remains in that state. A session stuck for 2 hours generates 120 notifications. This is the exact spam pattern the dedupe window was designed to prevent, but the time-based window is insufficient for persistent state conditions.
**Fix:** For reactions where `trigger_version` cannot be derived (state-based triggers only), extend the deduplication window to match the `escalation_delay_ms` for that reaction type. Alternatively, add a `"once-per-state-entry"` flag to `RetryState` that records the session's last state transition timestamp and skips delivery if the session has not re-entered the trigger state since the last successful delivery.

---

## Individual Findings — CC (Claude Code)

### MEDIUM · `all-complete` fires on manual `killed` sessions
**Section:** Component 4 — Poll Loop Integration, `all-complete` detection
**Issue:** The `all-complete` trigger condition is "ALL currently-tracked issues for the project are in terminal states and at least one transitioned to terminal this tick." `killed` is a terminal state, so manually killing a stuck session to unblock a blocker would trigger `all-complete` even if other issues remain unworked. The intent of `all-complete` is "all work is done," not "all sessions have stopped for any reason."
**Fix:** Scope `all-complete` to transitions into naturally-completed terminal states (`cleanup`, `archived`) rather than all terminal states. Or document explicitly that `killed` counts as terminal for `all-complete` purposes (intentional design choice, possibly acceptable).

### LOW · Unmapped priority labels have no default fallback
**Section:** Component 3 — Scheduler, dispatch ordering
**Issue:** "Priority rank is derived from the tracker's priority field — a configurable mapping from tracker priority labels to integer ranks." If the tracker returns a label not present in the mapping (e.g., a custom label), the behavior is undefined — could cause a sort panic or silent default to 0 (which would make the issue the highest priority).
**Fix:** Define an explicit fallback rank (e.g., `u32::MAX` — lowest priority) for unmapped labels. Document the fallback in the config field description.

---

## Individual Findings — Gemini

### MEDIUM · `all-complete` could fire on project initialization (empty state)
**Section:** Component 4 — Poll Loop Integration
**Issue:** "ALL currently-tracked issues for the project are in terminal states" is vacuously true when the `SessionStore` is empty (0 issues tracked) or when the project is first initialized. The "at least one transitioned to terminal this tick" guard partially mitigates this, but the more precise condition should be: active session count transitions from >0 to 0 AND the project has at least one issue in a terminal state.
**Fix:** Refine the condition: fire `all-complete` only when the count of non-terminal sessions transitions from ≥1 to 0 within the same tick, and at least one terminal session exists. This ensures the notification represents completion of a batch rather than an empty-start state.

### MEDIUM · `trigger_version` availability not mandated from tracker plugins
**Section:** Component 2 — Idempotency via Action Journal
**Issue:** The dedupe key includes `trigger_version` (CI run ID, commit SHA, review thread ID), but it is not explicitly stated that the `PollContext` (Phase ②) is required to capture these identifiers. If a tracker or CI plugin does not provide a unique identifier for a failure event, the idempotency logic falls back to the time-based window, which CF-2 above shows is insufficient for persistent states.
**Fix:** Add a mandate to ADR-0006 (Tracker) or the `PollContext` definition: plugins MUST provide a `trigger_version` identifier for all event-based reactions (CI runs, review decisions, PR state changes). For reactions that inherently lack a versioned trigger (state-only reactions like `stuck`), document that the time-window fallback applies and account for it in the CF-2 fix.

### LOW · Scheduler starvation by `stuck`/`needs_input` sessions
**Section:** Component 3 — Scheduler
**Issue:** If `maxConcurrentAgents` is reached by sessions in `stuck` or `needs_input` states, no new sessions can be spawned. The scheduler halts indefinitely. No automated "reaping" logic is mentioned.
**Fix:** Document whether/how `stuck` leads to a terminal state via timeout (global edge in ADR-0001). If it does, the scheduler will self-heal. If not, specify a configurable `stuckSessionTimeoutMs` after which the session is auto-killed, freeing the slot.

### LOW · Priority mapping defaults unspecified (same as CC LOW finding)
**Section:** Component 3 — Scheduler
**Fix:** Define a default fallback rank for unmapped labels (e.g., lowest priority = `u32::MAX`).

---

## Council Recommendations

In priority order:

1. **[Required] Add stale-reaction invalidation to the delivery path (CF-1):** Add `trigger_state: SessionStatus` to `PendingReaction`. Before delivering any queued reaction, verify the session's current state still matches. Discard if the condition has resolved. This is the only way to prevent spurious mid-loop fixes on already-resolved conditions.

2. **[Required] Fix notify spam for state-based triggers (CF-2):** For non-versioned reactions (`stuck`, `needs_input`, `agent-exited`), extend the dedupe window to `escalation_delay_ms` or implement a "once-per-state-entry" guard using the session's state transition timestamp. A per-minute notification rate for a stuck session is user-hostile.

3. **[Should] Mandate `trigger_version` from tracker plugins (Gemini MEDIUM):** Document in ADR-0006 or `PollContext` that plugins must provide versioned identifiers for event-based reactions. Clarify the fallback behavior for state-only reactions.

4. **[Should] Define unmapped priority label fallback (CC/Gemini LOW):** Specify `u32::MAX` (lowest priority) as the default fallback rank for unmapped labels.

5. **[Consider] Scope `all-complete` to naturally-completed terminal states or refine the trigger guard (CC MEDIUM + Gemini MEDIUM):** Either restrict to non-`killed` terminal states or adopt Gemini's "transitions from ≥1 to 0 non-terminal sessions" condition for greater precision.

---

*Note: Codex was excluded from this council session due to a stdin delivery failure — same root cause as round 1. Confirmed Findings are from CC + Gemini only.*

---

## Late CC Review Findings (Background Agent — Arrived After Initial Report)

A background CC review agent completed after the initial report and ADR revision. Its findings cover issues introduced by the round 1 revisions. Findings addressed in the round 2 ADR revision are marked ✅.

| Severity | Finding | Section | Status |
|----------|---------|---------|--------|
| HIGH | `tracker-terminal` row described kill+destroy as entry action work, violating ADR-0001 pure-graph-walk constraint | Component 1, reaction table | ✅ Fixed — table cell updated: kill+destroy are lifecycle engine transition side effects; reaction engine sends `notify` only |
| MEDIUM | `trigger_version` sourcing not enumerated per reaction type (`merge-conflicts` uses PR head SHA as stable proxy; `bugbot-comments` uses comment ID) | Component 2, idempotency | ✅ Fixed — added `trigger_version` source column to reaction table |
| MEDIUM | `maxRetriesPerIssue` journal scan is per-session; cross-session aggregation not specified | Component 3, criterion 6 | ✅ Fixed — specified cross-session scan of all sessions sharing `ISSUE_ID` (active + archived); bounded by O(`maxRetriesPerIssue`) |
| MEDIUM | Tracker-native sorting may not align with `priority_rank` mapping (GitHub has no native priority field) | Component 3, candidate set | ✅ Fixed — documented as best-effort approximation; noted GitHub limitation; guidance for teams using label-based priority |
| LOW | `all-complete` detection iterates sessions (not unspawned issues) — may fire while unspawned blocked issues remain | Component 4, `all-complete` detection | Documented limitation — `all-complete` fires when all *spawned* sessions complete; unspawned issues are not tracked. This is the intended scope: "all work the orchestrator has touched." Not fixed — acceptable by design. |
| LOW | `ReactionAction::AutoMerge` as escalation action is semantically invalid; no validation rule | Component 5, config | Not fixed — deferred to ADR-0003's two-pass validation implementation note. Will be caught at config load time. |
| LOW (partial) | `rework-requested` still listed as one of "11 Reaction type defaults" without visual distinction | Component 1, reaction table | ✅ Fixed — separated into MVP table (10 rows) and post-MVP table; `rework-requested` moved to post-MVP table |
