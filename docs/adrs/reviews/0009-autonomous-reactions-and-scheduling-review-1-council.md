# Council Review — ADR-0009: Autonomous Reactions and Scheduling

**Review round:** 1
**Date:** 2026-03-11
**Reviewers:** CC (Claude Code) · Gemini · Codex — excluded (stdin delivery failure)

---

## Council Verdict

ADR-0009 is a well-structured and thorough document that clearly motivates the joint FR4+FR5 scope, reasons through ten discrete alternatives, and produces a coherent design with five named components. The integration with prior ADRs is explicit and the Consequences section is candid. However, the ADR cannot be accepted as written. CC and Gemini together confirm three findings of High/Medium severity: (1) `rework-requested` has no `ReactionAction` enum variant and no confirmation gate for its destructive behavior — both CC and Gemini flag this; (2) the crash-recovery argument for the in-memory queue is incorrect — entry actions fire on transitions, not steady-state, so sessions already in `ci_failed` at crash time lose their pending reactions; (3) the scheduler complexity is misstated as O(active sessions) when it is O(all issues in active tracker states), creating a rate-limit risk at scale confirmed by both reviewers. Additional CC findings cover dedupe key gaps, config inheritance ambiguity, and a non-serializable `Instant` field. **Recommendation: Revise before accepting.**

---

## Confirmed Findings *(CC + Gemini)*

### CF-1 · High · `rework-requested` undefined and unguarded
**Flagged by:** CC (HIGH-1) · Gemini (MEDIUM-4)
**Section:** Component 1 — Reaction Engine / Reaction table + `ReactionAction` enum
**Issue:** The reaction table lists `rework-requested` with action "Close PR, fresh branch from default, re-spawn." The `ReactionAction` enum defines only `SendToAgent`, `Notify`, and `AutoMerge` — no `Rework` variant. Additionally, this is the most destructive automated action in the system, with no confirmation gate. Human contributions pushed to the agent's branch could be silently lost.
**Fix:** Add `ReactionAction::Rework` with explicit semantics (close PR, create branch, call `orchestrator.spawn_session`), add a `require_confirmation: true` config flag defaulting to `true`, and document double-idempotency. Or mark `rework-requested` as post-MVP and remove it from the MVP reaction table.

### CF-2 · High · Scheduler complexity misstated; rate-limit risk at scale
**Flagged by:** CC (MEDIUM-2) · Gemini (HIGH-1)
**Section:** Component 3 — Scheduler / Dispatch eligibility criteria + Consequences
**Issue:** The ADR describes scheduling as O(active sessions), but the scheduler must evaluate all issues in active tracker states — not just those with running sessions — to find eligible dispatch candidates. With 500+ open issues, `Tracker::get_issue()` calls per tick will exhaust API rate limits and push tick duration past the re-entrancy guard window.
**Fix:** Either (a) run the scheduling phase every N ticks (e.g., every 5 minutes), (b) limit pre-fetch to the top 20–50 issues by tracker-native sorting, or (c) rely on tracker-provided `is_blocked` flags and limit blocker graph depth to 1 level. Correct the O-complexity claim.

### CF-3 · Medium · Idempotency window blocks new CI failure context on rapid cycling
**Flagged by:** CC (MEDIUM-1) · Gemini (MEDIUM-2)
**Section:** Component 2 — Idempotency via Action Journal
**Issue:** The 5-minute time-based deduplication window, combined with a dedupe key of `(action_type, target_id)` that omits `reaction_type`, causes two problems: (1) if CI fails again within 3 minutes, new failure context is silently dropped, leaving the agent without information to iterate; (2) `ci-failed` and `changes-requested` reactions producing the same action on the same session within the window collide, dropping the second.
**Fix:** Change dedupe key to `(reaction_type, action_type, target_id, trigger_version)` where `trigger_version` is a change-specific identifier (CI build ID or commit SHA). Update the journal format accordingly.

---

## Individual Findings — CC (Claude Code)

### HIGH-1 · Section "1. Reaction Engine" / Reaction table + `ReactionAction` enum
**Severity:** High
**Issue:** The reaction table lists `rework-requested` with the action "Close PR, fresh branch from default, re-spawn." The `ReactionAction` enum in Section 5 defines only three variants: `SendToAgent`, `Notify`, and `AutoMerge`. There is no `Rework` variant. The enum is either incomplete or the reaction table is aspirational. The Consequences section acknowledges the double-idempotency requirement for rework, but the missing action type means the implementation contract is undefined. Implementors will face an unresolved gap.
**Fix:** Either add a `Rework` variant to `ReactionAction` (with its own semantics — close PR, create branch, call `orchestrator.spawn_session`) or explicitly mark `rework-requested` as a post-MVP stub and exclude it from the reaction table's MVP column.

---

### HIGH-2 · Section "Considered Options — Option 4" / Crash recovery re-derivation claim
**Severity:** High
**Issue:** The ADR justifies the in-memory queue (Option 4) with: "on restart, the poll loop re-evaluates all non-terminal sessions, entry actions re-fire for sessions already in reaction-triggering states." This is incorrect. Entry actions fire on *state transitions* (entering a state), not on polling a session that is already in that state. A session that was in `ci_failed` before the crash will not transition into `ci_failed` again on the next poll tick — it is already there. The lifecycle engine's graph walk will evaluate the session, find no triggering transition, and fire no entry action. The reaction queue will be empty on restart for all sessions that were mid-reaction-delivery at crash time.
**Fix:** Either (a) acknowledge that crashed pending reactions are lost and assess whether that is acceptable (the reaction will re-fire if a *new* CI run fails, but not for the current already-failed run), or (b) add a startup reconciliation step that scans all non-terminal sessions in reaction-triggering states and re-enqueues reactions if no recent successful delivery entry exists in the action journal. Option (b) is the correct fix for the stated intent.

---

### MEDIUM-1 · Section "2. Idempotency" / Dedupe key missing `reaction_type`
**Severity:** Medium
**Issue:** The deduplication key is defined as `(action_type, target_id)`. If two different reaction types (e.g., `ci-failed` and `changes-requested`) both produce a `send-to-agent` action targeting the same session within the 5-minute window, only the first will execute. The second will be silently skipped. This is a correctness issue: a code review comment that arrives while a CI fix is in flight would be dropped.
**Fix:** Change the dedupe key to `(reaction_type, action_type, target_id)`. Update the journal format to include `reaction_type` as a field.

---

### MEDIUM-2 · Section "3. Scheduler" / Blocker API calls unbounded
**Severity:** Medium
**Issue:** Dispatch eligibility criterion 5 states: "No non-terminal blocker issues (blockers are checked via `Tracker::get_issue()` on each blocker ID)." For issues with many blockers, this is an unbounded number of tracker API calls per tick per eligible issue. No cap, cache, or batch strategy is specified. Combined with the existing concern about tick duration (Consequences, first negative), this could cause ticks to exceed 30s consistently on projects with many blocked issues.
**Fix:** Specify a cap (e.g., check at most N blockers per issue per tick, or use `last_known_tracker_state` cached from the gather phase for issues that have their own active sessions). Alternatively, add blocker states to a cache warmed during the gather phase to avoid duplicate fetches.

---

### MEDIUM-3 · Section "Consequences — Negative" / `last_known_tracker_state` left unresolved
**Severity:** Medium
**Issue:** The Consequences section identifies that `SessionMetadata` does not currently store `last_known_tracker_state` and that the implementation "should cache" it to avoid extra tracker API calls in the schedule phase. This is framed as an implementation note but it directly affects correctness of per-state concurrency limits. Without caching, either (a) extra API calls are made (degrading performance) or (b) the per-state limit cannot be enforced (degrading scheduling correctness). The ADR defers the resolution rather than deciding.
**Fix:** Make a decision: either add `last_known_tracker_state: Option<TrackerState>` to `SessionMetadata` as part of this ADR's schema (updated during the gather phase), or explicitly accept the extra API call cost and bound it with a session-count cap.

---

### MEDIUM-4 · Section "5. Configuration" / Override merge semantics underspecified
**Severity:** Medium
**Issue:** Per-project `reactions` overrides are described as "project-level keys win," but the semantics for partially-overridden `ReactionConfig` structs are unspecified. If a project overrides `action` but not `max_retries`, does `max_retries` inherit the global value or the struct default (which may differ)? This ambiguity will cause inconsistent behavior across different override patterns.
**Fix:** Specify the merge strategy explicitly. Options: (a) field-level inheritance — each field independently falls back to the global value if absent from the project config, or (b) struct-level replacement — a project override replaces the entire `ReactionConfig` for that reaction type, with absent fields using struct defaults. State which is chosen and why.

---

### LOW-1 · Section "1. Reaction Engine" / `all-complete` queue key unspecified
**Severity:** Low
**Issue:** The `all-complete` detection enqueues a reaction "for the project," but `ReactionEngine.pending` is `HashMap<SessionId, VecDeque<PendingReaction>>` — keyed by session ID. A project-level reaction has no natural `SessionId`. The queue key for project-level reactions is never specified (sentinel value? project ID as a pseudo-session ID?).
**Fix:** Either use a sentinel `SessionId` convention (e.g., `"{project_id}-all-complete"`) or introduce a separate `project_pending: HashMap<ProjectId, VecDeque<PendingReaction>>` field. Document the choice in the struct.

---

### LOW-2 · Section "1. Reaction Engine" / `queued_at: Instant` not serializable
**Severity:** Low
**Issue:** `PendingReaction.queued_at` is typed as `Instant`. `Instant` is monotonic, platform-specific, and not serializable. The Consequences section states that `RetryState` is rebuilt from the action journal on restart. If any retry logic depends on `queued_at` (e.g., escalation delay), the timestamp cannot be reconstructed from the journal. Additionally, journal entries use `timestamp_ms` (wall time), creating an inconsistency.
**Fix:** Change `queued_at` to `SystemTime` or `u64` (Unix milliseconds). Escalation delay calculations should use wall-clock time to survive restarts and clock corrections.

---

## Council Recommendations

In priority order:

1. **[Required] Fix crash-recovery argument (HIGH-2):** Add a startup reconciliation step (`reconcile_pending_reactions()`) that scans all non-terminal sessions and re-enqueues reactions for any session in a reaction-triggering state with no recent successful journal entry. This replaces the incorrect "re-derivation via re-poll" claim. This is the most critical fix — the stated crash-safety guarantee does not hold as written.

2. **[Required] Resolve `rework-requested` action type gap (HIGH-1):** Either add `ReactionAction::Rework` with explicit semantics (close PR, create branch, call `spawn_session`), or demote `rework-requested` to a post-MVP stub with a `// TODO(post-mvp)` annotation. The action table must match the enum.

3. **[Should] Fix dedupe key to include `reaction_type` (MEDIUM-1):** Update the dedupe key to `(reaction_type, action_type, target_id)` and the journal format to include `reaction_type`. This prevents silent drops when two reaction types produce the same action on the same target within the dedup window.

4. **[Should] Decide and document `last_known_tracker_state` caching (MEDIUM-3 + MEDIUM-2):** Add `last_known_tracker_state: Option<String>` to `SessionMetadata`, updated in the gather phase. This simultaneously resolves the per-state concurrency limit correctness gap and bounds blocker-check API call volume (blockers with active sessions can use cached state).

5. **[Should] Specify config override merge semantics (MEDIUM-4):** Choose and document field-level vs. struct-level override semantics for per-project `ReactionConfig`. A one-sentence addition to Section 5 suffices.

---

### Gemini Individual Findings

**MEDIUM · Retry state re-derivation fragility**
*Section: Component 2 — Idempotency / action journal format*
Rebuilding `RetryState` by parsing the text-based action journal on restart is fragile as journals grow. Parsing timestamps and "failed" strings from an append-only log is error-prone.
*Fix:* Use JSONL/NDJSON format for the action journal with a typed parser, or maintain a small `last_reaction_results.json` per session for fast retry state reconstruction.

**LOW · Blocker detection depth unbounded**
*Section: Component 3 — Scheduler, criterion 5*
Recursive blocker checking via `Tracker::get_issue()` may be expensive for deep blocker graphs.
*Fix:* Limit blocker depth to 1 level (direct blockers only) or use tracker-native `is_blocked` flags where available.

**LOW · `all-complete` notification noise**
*Section: Component 4 — Poll Loop Integration*
Firing `all-complete` every time any session in a project hits terminal state could generate high notification volume.
*Fix:* Only fire when all currently-tracked issues for the project are terminal, with a cooldown (e.g., 1 hour) to prevent re-firing.

---

*Note: Codex was excluded from this council session due to a stdin delivery failure — the CLI received empty input. Confirmed Findings are from CC + Gemini only.*
