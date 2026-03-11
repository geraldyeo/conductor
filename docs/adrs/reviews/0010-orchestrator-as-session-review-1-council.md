# Council Review — ADR-0010: Orchestrator-as-Session

**Review round:** 1
**Date:** 2026-03-11
**Reviewers:** CC (Claude Code) · Gemini · Codex — excluded (stdin delivery failure)

---

## Council Verdict

ADR-0010 is a focused, well-motivated document that correctly decomposes the orchestrator-as-session feature into four components and cleanly resolves all prior ADR dependencies. However, it cannot be accepted as written. CC and Gemini together confirm three findings: (1) worker sessions can issue `ao session kill {prefix}-orchestrator` — no isolation layer prevents worker-on-orchestrator interference before FR17 lands (confirmed by both reviewers); (2) using the project root directly causes git lock contention with human developers (CC flags it as underspecified, Gemini flags the concrete lock risk); (3) the orchestrator prompt contains only static data with no initial session state, leaving the agent flying blind at spawn and requiring immediate shell commands to orient itself (both reviewers flag this from different angles). CC additionally identifies two High findings: an incomplete state machine (no `errored→spawning` restart arc) and an unspecified cold-start recovery for orchestrator sessions found in `errored` at daemon restart. Gemini adds a critical High: the ADR defines a circuit breaker for *restarts* but no limit on orchestrator-initiated *spawns*, enabling a spawn storm if the agent hallucinates. **Recommendation: Revise before accepting.**

---

## Confirmed Findings *(CC + Gemini)*

### CF-1 · High · No action-rate limit on orchestrator-initiated spawns (spawn storm risk)
**Flagged by:** Gemini (HIGH-1) · CC (implicit in circuit-breaker scope)
**Section:** Decision / Component 3 — Lifecycle Engine Integration
**Issue:** The circuit breaker limits orchestrator *restarts* (3 per hour), but places no limit on orchestrator-initiated `ao spawn` *calls*. A hallucinating or logic-looping orchestrator agent could issue dozens of spawn commands per minute, exhausting global concurrency limits, incurring significant API costs, and consuming all tracker issues before a human can intervene. The orchestrator daemon's global `maxConcurrentAgents` is a concurrency cap, not a rate limit — it prevents simultaneous sessions but not a rapid burst-then-complete spawn pattern.
**Fix:** Add an `orchestratorSpawnRateLimitPerMinute` config field (default: 5) enforced by the IPC handler for `Spawn` and `BatchSpawn` requests originating from the orchestrator session. Requests exceeding the rate limit are rejected with an error returned to the orchestrator agent's shell, causing it to back off.

### CF-2 · Medium · Worker-on-orchestrator interference before FR17
**Flagged by:** CC (MEDIUM-1 — security boundary) · Gemini (MEDIUM — isolation between worker and orchestrator)
**Section:** Context / Decision — Component 4 Orchestrator Prompt
**Issue:** Worker sessions share the same machine and IPC socket. A worker agent can run `ao session kill {prefix}-orchestrator` before FR17's mechanical enforcement lands. There is no interim isolation layer. The ADR acknowledges FR17 is deferred but does not specify a stop-gap.
**Fix:** Document an interim mitigation: the IPC handler checks the requesting session's metadata (`IS_ORCHESTRATOR` flag) for lifecycle mutations (Kill, Stop). Requests from non-orchestrator sessions targeting the orchestrator session ID are rejected with a permission error. This is a soft guard implementable without FR17's full scoped-credential mechanism.

### CF-3 · Medium · Project root workspace causes git lock contention
**Flagged by:** CC (LOW-9 — underspecified) · Gemini (MEDIUM — `.git/index.lock` risk)
**Section:** Decision / Component 2 — Spawn Sequence, step 2
**Issue:** Using the project root directly (rather than a dedicated worktree) creates `.git/index.lock` and other resource locks when the orchestrator agent runs git-touching tools (`git status`, file indexing by Claude Code). This interferes with human developers working in the same directory, even in `read-only` sandbox mode.
**Fix:** Create a dedicated read-only worktree for the orchestrator session at spawn time (e.g., `~/.agent-orchestrator/{hash}/orchestrator-workspace/`), checked out to the default branch. This eliminates lock contention while preserving cross-project read access via symlinks or direct path references.

### CF-4 · Medium · Orchestrator prompt contains no initial session state snapshot
**Flagged by:** CC (MEDIUM-2 — prompt staleness) · Gemini (LOW — agent "flying blind")
**Section:** Decision / Component 4 — Orchestrator Prompt
**Issue:** The orchestrator prompt includes only static data (command reference, project list). The agent has no knowledge of active sessions at spawn time and must immediately run `ao status --json` to orient itself — adding latency and token cost before any coordination action. If the agent's initial `ao status` call fails, it has no fallback context.
**Fix:** Include a snapshot of `ao status --json` output in the rendered prompt (captured at spawn time). Exclude mutable runtime constraints (concurrency limits, tracker states) from the static prompt layer — these change via hot-reload and should be queried live. This gives the agent a bootstrap context without coupling the prompt to mutable config.

---

## Individual Findings — CC (Claude Code)

### HIGH-1 · Section "3. Lifecycle Engine Integration" — Incomplete orchestrator state machine and undefined `errored` semantics
**Severity:** High
**Issue:** The evaluate phase table shows only three transitions: `spawning→working`, `working→errored`, and `any→killed`. The auto-restart mechanism (component 3, transition phase) describes an entry action on `errored` that queues a restart — but the state transition that fires after the restart completes is never specified. The session must transition from `errored` back to `spawning` (or to a new attempt); this arc is absent from the table. Additionally, the semantics of `errored` for orchestrator sessions are inconsistently defined relative to worker sessions: for workers, `errored` is effectively terminal (human intervention required); for the orchestrator, it is a transient state pending a scheduled restart. This duality is not reconciled in the ADR. The spawn sequence (component 2, step 1) states "skip spawn if a session already exists and is non-terminal" — if `errored` counts as non-terminal for orchestrator sessions (because it will auto-restart), then the idempotency check must know the session type before evaluating terminality. This is a semantic gap that will produce ambiguous implementation choices.
**Fix:** Add the `errored→spawning` restart arc to the evaluate phase table. Define explicitly whether `errored` is terminal or non-terminal for orchestrator sessions (recommendation: non-terminal, distinct from worker `errored`). Update step 1 of the spawn sequence to clarify that `errored` orchestrator sessions are considered non-terminal and trigger the restart path rather than the skip path.

---

### HIGH-2 · Section "2. Spawn Sequence" — Race condition between daemon restart and `errored`-state idempotency
**Severity:** High
**Issue:** Component 3 states: "The restart executes as a scheduled task within the poll loop at the next eligible tick, re-running the spawn sequence (component 2, step 1's idempotency check is skipped on restart)." But this skip is only described for restarts triggered by the running daemon. If the orchestrator daemon itself crashes while the orchestrator session is in `errored` state (mid-backoff window), the daemon's restart recovery path re-polls all sessions. Component 2, step 1's idempotency check will see a session in `errored`. Whether `errored` is treated as non-terminal (see HIGH-1) determines what happens next — but the ADR does not specify this case. If `errored` is treated as terminal by the cold-start path, the orchestrator session will never be respawned after a daemon crash during a backoff window. If treated as non-terminal, the idempotency check would skip the spawn, leaving the session stuck in `errored`. Neither outcome is correct; the cold-start recovery path for orchestrator sessions in `errored` is unspecified.
**Fix:** Add a startup reconciliation step (analogous to ADR-0009's crash recovery) that specifically handles orchestrator sessions in `errored` state: re-enqueue the restart (bypassing the circuit breaker check if the crash occurred within the backoff window, or re-evaluating against `RESTART_COUNT` and `LAST_RESTART_AT` metadata if the window has passed). Document this in the lifecycle integration component.

---

### MEDIUM-1 · Section "3. Lifecycle Engine Integration" — No liveness detection for hung-but-alive orchestrator agent
**Severity:** Medium
**Issue:** The evaluate phase removes `stuck` detection with the correct rationale (the orchestrator is frequently idle between tasks). However, this creates a blind spot: if the orchestrator agent process is alive (`Runtime::is_alive()=true`) but is hung — waiting indefinitely on a tool call, blocked on network I/O, or deadlocked — the lifecycle engine will never detect the problem. `working→errored` only fires when `is_alive()` returns false. A hung Claude Code process may remain alive at the OS level while being functionally unresponsive for hours. Workers have `stuck` detection as a fallback; the orchestrator has none.
**Fix:** Introduce a configurable `orchestratorActivityTimeoutMs` (default: much larger than worker stuck timeout, e.g., 3600000 = 1 hour, to avoid false positives during legitimate idle periods). If `Agent::detect_activity()` reports `idle` continuously for this duration, treat the orchestrator session as stuck and transition to `errored` (triggering the auto-restart path). This is distinct from worker stuck detection in that it uses a longer threshold and routes through restart rather than human escalation.

---

### MEDIUM-2 · Section "4. Orchestrator Prompt" — Prompt staleness on config hot-reload
**Severity:** Medium
**Issue:** `PromptEngine::render_orchestrator()` is called once at session spawn (component 2, step 3). The prompt includes "Current project list — Project IDs, repo names, active tracker states, and concurrency limits from Config." ADR-0003 defines config hot-reload: changes are applied without a daemon restart. If `maxConcurrentSessions`, project names, or concurrency limits change via hot-reload after the orchestrator session is running, the orchestrator agent's prompt will reflect the stale configuration. The agent could make dispatch decisions (e.g., spawning a new session) that violate the updated concurrency limits it was never told about.
**Fix:** Either (a) describe a mechanism to re-deliver an updated context snapshot to the running orchestrator session when relevant config fields change (e.g., send a structured update via `ao send`), or (b) explicitly scope the "current project list" layer to static config fields only (names, repo paths) and exclude mutable runtime constraints (concurrency limits, tracker states) from the prompt — instructing the orchestrator agent to query current state via `ao status --json` rather than relying on the initial prompt. Option (b) is simpler and more robust.

---

### MEDIUM-3 · Section "Decision" — Circuit breaker reset semantics underspecified
**Severity:** Medium
**Issue:** The circuit breaker is defined as "max 3 restarts per hour" via `maxOrchestratorRestarts` (default: 3) and `orchestratorRestartWindowMs` (default: 3600000). The ADR does not specify whether this is a sliding window or a fixed hourly bucket, and does not specify when or how the counter resets after a successful run. If the orchestrator runs cleanly for 59 minutes and then crashes three times in one minute, the circuit breaker trips. After the window expires, does the counter reset automatically? Is `RESTART_COUNT` in metadata a lifetime counter or a window counter? `LAST_RESTART_AT` is stored in metadata but the ADR never specifies the algorithm for evaluating the window against these two fields.
**Fix:** Specify the exact algorithm: e.g., "Count the number of restarts where `LAST_RESTART_AT > now - orchestratorRestartWindowMs`. If count >= `maxOrchestratorRestarts`, trip the breaker. The count is derived from metadata on each restart attempt, not from a separate in-memory counter." This makes the circuit breaker crash-safe and eliminates ambiguity. Alternatively, store a `RESTART_TIMESTAMPS` list (last N entries) in metadata and count entries within the window.

---

### LOW-1 · Section "1. Session Identity and Metadata" — `orchestratorSessionPrefix` derivation is order-dependent
**Severity:** Low
**Issue:** The `orchestratorSessionPrefix` config field "defaults to the first project's prefix." In a multi-project config, "first project" depends on config file order. YAML map keys are nominally unordered (though `serde_yml` preserves insertion order). If a user reorders their project list, the orchestrator session ID changes, breaking any external references (dashboards, scripts, notifications) that depend on the stable `{prefix}-orchestrator` name. The ADR's claim that the session ID is "stable and unique per orchestrator instance" is undermined by this order dependency.
**Fix:** Either require `orchestratorSessionPrefix` to be an explicit required field when multiple projects are present (fail validation if absent and multiple projects exist), or define the default as the lexicographically first project prefix rather than config-insertion-order first. Document the chosen rule in ADR-0003's config schema.

---

### LOW-2 · Section "2. Spawn Sequence, step 4" — `PromptDelivery::PostLaunch` rationale not stated
**Severity:** Low
**Issue:** Step 4 mandates `PromptDelivery::PostLaunch` for the orchestrator session without explanation. For worker sessions this choice is motivated by Claude Code's interactive mode requirement. The same rationale applies here, but a reader reviewing only ADR-0010 cannot discern why `Inline` was excluded. This creates an undocumented constraint that future maintainers might change without understanding the implication.
**Fix:** Add a one-sentence rationale: "PostLaunch is required because the claude-code agent plugin starts in interactive mode; the system prompt cannot be passed as a CLI argument." This matches the worker session rationale in ADR-0004.

---

### LOW-3 · Section "Decision" — Option 5 (MCP tools) has no tracking reference
**Severity:** Low
**Issue:** The decision section notes Option 5 (structured MCP tool definitions) is "listed as a planned post-MVP enhancement" but assigns no FR number, ADR placeholder, or tracking mechanism. Unlike other deferred items in prior ADRs which reference specific FR numbers, this enhancement is unanchored.
**Fix:** Assign a tracking reference. Either map it to an existing FR (FR17 handles mutation authority; MCP tools for `ao` commands could fall under FR17's mechanical enforcement scope) or create a brief note in `docs/plans/` as a placeholder. This prevents the enhancement from being forgotten.

---

## Council Recommendations

In priority order:

1. **[Required] Complete the orchestrator state machine (HIGH-1):** Add the `errored→spawning` restart arc to the evaluate phase table. Explicitly classify `errored` as non-terminal for orchestrator sessions and distinguish its semantics from the worker `errored` state. The step 1 idempotency check in the spawn sequence must account for this classification.

2. **[Required] Define cold-start recovery for orchestrator sessions in `errored` (HIGH-2):** Add a startup reconciliation case for orchestrator sessions found in `errored` state on daemon restart. Specify that `RESTART_COUNT` and `LAST_RESTART_AT` metadata are used to evaluate the circuit breaker before re-enqueuing the restart, making the restart logic crash-safe.

3. **[Should] Add orchestrator activity timeout (MEDIUM-1):** Introduce `orchestratorActivityTimeoutMs` (default 1 hour) to detect hung-but-alive orchestrator sessions. Route through the existing auto-restart path on timeout, preventing silent hangs that block autonomous coordination without any notification.

4. **[Should] Constrain prompt to static config fields only (MEDIUM-2):** Remove mutable runtime constraints (concurrency limits, active tracker states) from the initial orchestrator prompt. Instead, instruct the orchestrator agent to query `ao status --json` for live state. This eliminates the prompt staleness problem without requiring a re-delivery mechanism.

5. **[Should] Specify circuit breaker algorithm precisely (MEDIUM-3):** Define the exact sliding-window algorithm using `LAST_RESTART_AT` metadata. Clarify whether `RESTART_COUNT` is a lifetime counter or a window counter, and document the reset semantics.

---

### Gemini Individual Findings

**MEDIUM · `read-only` sandbox contradicts "create orchestration artifacts"**
*Section: Consequences — Negative, first bullet*
The ADR recommends defaulting the orchestrator sandbox to `read-only` to protect the main branch, but simultaneously states the agent may need to "create orchestration artifacts." These are mutually exclusive. If strictly read-only, artifact creation will fail silently or with an unhelpful error.
*Fix:* Specify a dedicated writable directory (e.g., `.conductor/orchestrator/`) that is explicitly permitted under a `sandbox: "scoped-write"` mode, or accept `workspace-write` and document the risk explicitly.

---

*Note: Codex was excluded from this council session due to a stdin delivery failure — the CLI received empty input. Confirmed Findings are from CC + Gemini only.*
