# Council Review — ADR-0010: Orchestrator-as-Session

**Review round:** 2
**Date:** 2026-03-11
**Reviewers:** CC (Claude Code) · Gemini · Codex

---

## Council Verdict

The revised ADR-0010 is a significant improvement over round 1 — it correctly completes the state machine, adds crash-safe startup reconciliation, specifies the circuit breaker algorithm, resolves the prompt flying-blind issue, and adds the spawn rate limit. These were all the round 1 confirmed findings. The document establishes a solid architectural foundation.

However, four confirmed findings remain across the three reviewers. **CF-1 (High)** — the IPC handler does not guard against spawning a clone orchestrator using the reserved session ID, confirmed by CC and Gemini. **CF-2 (High)** — the orchestrator worktree is described as checking out "the default branch" without specifying which repository in multi-project/multi-repo setups, confirmed by CC and Codex. **CF-3 (Medium)** — `IS_ORCHESTRATOR` is cased inconsistently (`is_orchestrator=true` in one place, `IS_ORCHESTRATOR=true` in others), confirmed by CC and Codex. **CF-4 (Medium)** — the default read-only sandbox blocks `git fetch` and `git checkout` which the document says the orchestrator can run, confirmed by CC and Gemini. **Recommendation: Address CF-1 through CF-4 before accepting.**

---

## Confirmed Findings *(2+ reviewers)*

### CF-1 · High · IPC handler does not guard against clone orchestrators
**Flagged by:** CC (HIGH) · Gemini (H-1)
**Section:** Component 3 — Lifecycle Engine Integration, IPC handler guards
**Issue:** The spawn sequence (component 2, step 1) checks for an existing non-terminal orchestrator session at startup, but this check is in the daemon's *own* startup path, not in the IPC `Spawn` handler. Any agent (or a compromised worker) that issues `ao spawn --session-id {prefix}-orchestrator` or passes `IS_ORCHESTRATOR=true` metadata via an IPC `Spawn`/`BatchSpawn` request would bypass this check entirely and create a second orchestrator session. This violates the single-instance identity rule and could allow a rogue worker to fork coordination authority.
**Fix:** Add explicit guards to the IPC `Spawn` and `BatchSpawn` handlers: (1) reject any request with a session ID matching `*-orchestrator` pattern, and (2) reject any request with `IS_ORCHESTRATOR=true` in the supplied metadata. Return a descriptive error: "reserved session identity — orchestrator sessions may only be spawned by the daemon lifecycle."

### CF-2 · High · Multi-project worktree repository is unspecified
**Flagged by:** CC (HIGH) · Codex (H-2)
**Section:** Component 2 — Spawn Sequence, step 2; Component 1 — Session Identity
**Issue:** The worktree is described as "checked out to the default branch" at `{data_path}/orchestrator-workspace/`. In a multi-project/multi-repo configuration — where one orchestrator manages issues across multiple repositories — it is not specified which repository's default branch this worktree tracks. An orchestrator session cannot be checked out to the default branch of multiple repos simultaneously. The ADR is therefore not directly implementable for the multi-project case without additional specification.
**Fix:** Specify the worktree's repository explicitly. Option A: the orchestrator worktree is tied to the "primary" project (the one providing `orchestratorSessionPrefix`, defaulting to the lexicographically first project ID). The orchestrator agent reads other project repos via absolute paths. Option B: the orchestrator workspace is a bare directory with no git worktree — the orchestrator agent calls `ao status --json` for live state and doesn't need a source checkout at all. Document which option is chosen and why.

### CF-3 · Medium · `IS_ORCHESTRATOR` casing inconsistency
**Flagged by:** CC (MEDIUM) · Codex (M-3)
**Section:** Context section (line 27) vs. Component 1 and Component 3
**Issue:** The Context section cross-referencing ADR-0005 uses `is_orchestrator=true` (lowercase key). Components 1 and 3 use `IS_ORCHESTRATOR=true` (uppercase key). Since metadata is stored and read as `KEY=VALUE` string pairs (ADR-0005), casing determines exact key lookup. An implementation using the lowercase spelling in metadata storage and the uppercase spelling in lookup will silently fail to detect orchestrator sessions, breaking the entire simplified lifecycle path.
**Fix:** Choose a canonical casing and use it consistently throughout the document. `IS_ORCHESTRATOR` (uppercase) is preferred since metadata keys in ADR-0005 examples use uppercase. Add a note to the Council Review Findings Addressed table at the end of the document.

### CF-4 · Medium · Git operations contradict read-only sandbox default
**Flagged by:** CC (MEDIUM) · Gemini (M-1)
**Section:** Consequences (negative), Component 2 step 2
**Issue:** The Consequences section states "The orchestrator worktree defaults to `sandbox: 'read-only'`" and then says the orchestrator agent "can run `git fetch` and `git checkout` to update." A strict read-only sandbox blocks all disk writes, including updates to `.git/FETCH_HEAD`, the object store, and the index file that `git fetch` and `git checkout` require. The two statements are contradictory under any standard sandbox implementation.
**Fix:** Choose one: (a) the default sandbox is `read-only` for *source code files only*, with `.git/` metadata writes explicitly permitted — document this as a special exception and note what the sandbox implementation must allow; or (b) change the default to `'workspace-write'` with a note that this allows the orchestrator agent to modify the working tree (not just `.git/` metadata). Option (a) is preferred as it limits blast radius while enabling git operations.

---

## Individual Findings — CC (Claude Code)

### MEDIUM · Shutdown guard for auto-restart missing
**Section:** Component 3 — Transition phase, auto-restart on `errored`
**Issue:** If the orchestrator session transitions to `errored` while `ao stop` is executing a graceful shutdown, the `errored` entry action schedules an auto-restart. The restart is enqueued as a scheduled task in the poll loop. Depending on poll loop re-entrancy timing, the restart may fire before the daemon finishes shutting down, spawning a new orchestrator session that immediately loses its daemon. The ADR does not address this interaction.
**Fix:** The `errored→spawning` restart path should first check whether a daemon shutdown is in progress (e.g., read the `shutdown_tx` signal from ADR-0007). If shutdown is pending, skip the restart and log "restart suppressed: daemon shutting down."

---

## Individual Findings — Gemini

### MEDIUM · Worktree staleness risk
**Section:** Consequences (negative), Component 3 Gather phase
**Issue:** The orchestrator worktree is checked out at spawn time and does not track branch updates automatically. Since the orchestrator's primary role is monitoring and making coordination decisions, a stale view of the codebase could cause it to misinterpret a fix or regression. The agent *can* update itself, but the design relies on the agent proactively calling `git fetch` with no triggering mechanism.
**Fix:** Consider having the lifecycle engine's gather phase for the orchestrator session run `git fetch --prune` in the orchestrator worktree periodically (e.g., every N ticks, configurable via `orchestratorGitSyncIntervalTicks`). This ensures the orchestrator's view stays current without requiring the agent to self-manage.

### LOW · Activity timeout conflates idle-waiting with unresponsive
**Section:** Component 3 — Evaluate phase, `working → errored` on activity timeout
**Issue:** The 1-hour `orchestratorActivityTimeoutMs` timeout assumes a continuously-idle orchestrator is hung. In a quiet project, the orchestrator may be legitimately idle for hours waiting for CI, PR reviews, or new issues. The current design would restart a healthy orchestrator unnecessarily in low-activity periods.
**Fix:** The `detect_activity()` implementation for orchestrator sessions should distinguish between "idle and waiting for events" (agent is running, tmux session has cursor, but no keystrokes) and "process unresponsive" (runtime not alive or shell frozen). Alternatively, document that `orchestratorActivityTimeoutMs` should be set conservatively (≥ 4 hours) or to 0 to disable for quiet projects.

---

## Individual Findings — Codex

### HIGH · `AO_SESSION` guard is spoofable
**Section:** Component 3 — IPC handler guards, worker-on-orchestrator guard
**Issue:** The worker-on-orchestrator kill/stop guard relies on `AO_SESSION` passed via caller process environment. This value is caller-controlled — any shell-capable process can set `AO_SESSION` to impersonate a worker session (or not set it at all). This does not provide a reliable authorization boundary for preventing workers from killing the orchestrator. The ADR acknowledges this as a "soft guard pending FR17" but rates it as providing protection against "worker-initiated kill/stop."
**Explanation:** The ADR is correct that this is a soft guard and that FR17 provides the full solution. The guard's value is deterrence (prevents accidental kill from confused agent), not security (does not prevent malicious or buggy agents). This should be explicitly framed in the ADR.
**Fix:** Reframe the guard's description: "This is a soft guard providing ergonomic protection against accidental worker-initiated kills — it does not provide a hard authorization boundary. FR17's scoped credential enforcement is the full solution."

### MEDIUM · Session archive mechanics under fixed ID unspecified
**Section:** Option 7 — Auto-restart with backoff; Component 3 — Transition phase
**Issue:** The ADR says "the old session is archived" and "incremented attempt counter in metadata" but the exact mechanics are underspecified: does archiving mean moving the session metadata to the `{hash}/archive/` directory (as per ADR-0005)? If the session ID is fixed, how does the archive directory avoid collision with the current session? What exactly constitutes "archive" vs. "the current entry" for a session that is being restarted?
**Fix:** Specify: on restart, the current session entry in `{hash}/sessions/{prefix}-orchestrator/` is preserved as the active entry (attempt counter incremented, status reset to `spawning`). No archiving occurs on restart — archiving only occurs on terminal destruction (`ao session kill` or circuit breaker trip). This aligns with ADR-0005's archive behavior (archive on delete, not on restart).

---

## Council Recommendations

In priority order:

1. **[Required] Add IPC spawn guard for reserved orchestrator identity (CF-1):** Add explicit rejection in `Spawn`/`BatchSpawn` IPC handlers for any session ID matching `*-orchestrator` or metadata containing `IS_ORCHESTRATOR=true`. This closes the clone-orchestrator vector.

2. **[Required] Specify multi-project worktree repository (CF-2):** Document which repository the orchestrator worktree tracks in multi-repo setups. The simplest answer: tie it to the `orchestratorSessionPrefix` project (same project that provides the prefix). Add a sentence to component 2 step 2.

3. **[Required] Fix `IS_ORCHESTRATOR` casing inconsistency (CF-3):** Standardize to `IS_ORCHESTRATOR=true` (uppercase) throughout. Fix the lowercase instance in the Context section cross-reference to ADR-0005.

4. **[Required] Resolve git operations vs read-only sandbox contradiction (CF-4):** Clarify that the read-only sandbox permits `.git/` metadata writes (git fetch) while blocking source code modifications. Update the worktree description and the Consequences section accordingly.

5. **[Should] Add shutdown guard to auto-restart path (CC MEDIUM):** Check daemon shutdown signal before enqueuing an `errored→spawning` restart to prevent a restart race during `ao stop`.

---

## Council Review Findings Addressed (Round 2)

All confirmed and high-priority findings addressed in this revision. See ADR-0010 "Council Review — Round 2 Findings Addressed" table for the full resolution summary.

---

## Late CC Review Findings (Background Agent — Arrived After Initial Report)

A background CC review agent completed after the initial report and ADR revision. Its findings cover issues introduced by the round 1 revisions. Findings addressed in the round 2 ADR revision are marked ✅.

| Severity | Finding | Section | Status |
|----------|---------|---------|--------|
| HIGH | `{data_path}` in the worktree path is a per-project FNV hash (`DataPaths::new()`); multi-project orchestrator has no canonical project to derive it from | Component 2, spawn sequence; Component 1 ADR-0005 cross-ref | ✅ Fixed — changed to `{orchestrator_root}` (global root `~/.agent-orchestrator/`); `DataPaths` gains `orchestrator_root()` accessor; orchestrator metadata lives outside per-project hash directories |
| MEDIUM | Precedence values 0–4 in orchestrator transition table may conflict with ADR-0001's global band namespace (band 2 = budget, band 3 = local edges start) | Component 3, evaluate phase table | ✅ Fixed — added explicit note: orchestrator precedence values are local-only, evaluated in an isolated branch (`IS_ORCHESTRATOR=true` gate), not in the shared band namespace |
| MEDIUM | `LAST_RESTART_AT` is redundant with `max(RESTART_TIMESTAMPS)`; split-write inconsistency risk | Component 1, session metadata | ✅ Fixed — `LAST_RESTART_AT` removed; `max(RESTART_TIMESTAMPS)` used directly for display |
| MEDIUM | `BatchSpawn` counting semantics unspecified — per-call vs per-session in rate limit | Component 3, spawn rate limit | ✅ Fixed — `BatchSpawn` counts N tokens (one per session); full batch rejected if limit exceeded; agent must split large batches |
| MEDIUM | `AO_SESSION` spoof vector not explicitly acknowledged (env variable can be unset via `env -u`) | Component 3, worker-on-orchestrator guard | Partially addressed in round 2 revision (reframed as soft ergonomic guard); spoof-via-unset vector not named explicitly. Acceptable — the FR17 reference covers the full fix. |
| LOW | `ao status --json` snapshot framing should explicitly warn agent it may be stale | Component 4, prompt layer 3 | Not fixed — deferred to prompt template content (outside ADR scope). |
| LOW | `orchestrator-artifacts/` directory lifecycle undefined (creation, cleanup, archival) | Consequences | Not fixed — deferred. Directory created at spawn if absent; preserved across restarts; archived on terminal session destruction. To be specified in implementation plan. |
| LOW | Two `working→errored` rows share destination; short-circuit behavior not stated | Component 3, evaluate phase table | ✅ Fixed — added note: evaluation short-circuits after first match; process-death (precedence 2) fires before activity-timeout (precedence 4); both routes lead to same `errored` entry action. |
