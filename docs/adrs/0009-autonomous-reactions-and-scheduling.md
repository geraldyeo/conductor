# ADR-0009: Autonomous Reactions and Scheduling

## Status
Accepted (revised — round 2 findings addressed)

## Context

The lifecycle engine (ADR-0001) detects state changes and fires entry actions, but reaction queuing and scheduling were explicitly deferred to post-MVP. The stub `queueReaction("ci-failed")` entry action has no consumer. The action journal (ADR-0005, FR15) exists per session but idempotency checks are unimplemented. Budget enforcement transitions sessions to `killed` (via global edges) but notifies no one.

This ADR covers two tightly coupled post-MVP features — FR4 (Autonomous Reactions) and FR5 (Scheduling & Concurrency) — because they share the same poll loop (ADR-0001), the same dispatch trigger, and the same retry/backoff machinery. Separating them would require re-designing integration points twice.

Key forces:

- The poll loop (30s default, re-entrancy guarded) is the natural trigger for both scheduling evaluations and reaction delivery. Adding background tasks with independent timers would create two clocks to synchronize.
- FR4 defines 11 reaction types, 3 action types (`send-to-agent`, `notify`, `auto-merge`), and a wait-for-ready protocol: non-terminal events must not interrupt an agent mid-turn.
- FR5 defines a global concurrency cap (`maxConcurrentAgents`), per-tracker-state limits (`maxConcurrentAgentsByState`), priority-based dispatch (highest priority → oldest first), and blocker/dependency exclusion.
- Idempotency is non-negotiable: the same poll cycle may re-evaluate entry actions for a session already in `ci_failed` state. Without deduplification, reactions fire repeatedly.
- The reaction engine's retry logic (max retries, exponential backoff) must survive orchestrator restarts. Retry state cannot be in-memory only.
- FR5 scheduling is stateless across ticks — eligible issues are re-evaluated each tick from tracker + session state. No queue to persist; re-evaluation is the recovery mechanism.

Six prior ADRs constrain the design:

1. **ADR-0001** — Entry actions (`queueReaction`, `notify`) are the reaction engine's inputs. The poll loop's three-phase structure (gather/evaluate/transition) gains a fourth phase: react+schedule. The 30s default tick governs reaction delivery latency.
2. **ADR-0003** — `reactions` and `maxConcurrentAgents` / `maxConcurrentAgentsByState` live in the existing config struct. Hot-reload applies to reaction rules and concurrency limits.
3. **ADR-0004** — `RuntimeStep::SendMessage` is the delivery primitive for `send-to-agent` actions. Activity state detection (`Agent::detect_activity`) is the gate for the wait-for-ready protocol.
4. **ADR-0005** — The action journal (per-session append-only log) provides the idempotency store. `SessionStore` provides `list()` for scheduler evaluation.
5. **ADR-0006** — `Tracker::get_issue()` and `classify_state()` provide issue priority and blocker state for FR5 dispatch decisions. `Tracker::add_comment()` implements workpad updates as part of reaction delivery.
6. **ADR-0007** — `OrchestratorRequest::Spawn` is the IPC message the scheduler sends to spawn new sessions. The `Orchestrator` struct's channel-based concurrency model (mpsc from IPC listener to poll loop) applies to scheduler-triggered spawns identically.

## Considered Options

### Reaction Engine Placement

1. **Inline in the lifecycle engine** — Transitions fire reactions directly as part of the transition phase. Simple, but couples state navigation to action execution. Guards must remain pure functions; mixing side effects into evaluation violates ADR-0001's design principle. Impossible to retry failed reactions without re-triggering the transition.

2. **Separate `ReactionEngine` module, enqueue/dequeue model** — The lifecycle engine's entry actions enqueue reaction events; the `ReactionEngine` dequeues and executes them in a dedicated poll-loop phase. Decouples detection from response. Entry actions remain fire-and-forget (as designed in ADR-0001). The reaction engine holds all retry state, idempotency logic, and wait-for-ready queuing independently.

3. **Separate process with its own poll loop** — A dedicated reaction service reads session state and acts. Clean process boundary, but introduces a second long-running process, coordination overhead, and a race between the lifecycle engine and the reaction service both reading/writing session state.

### Reaction Queue Durability

4. **In-memory queue** — Reactions are lost on orchestrator crash. On restart, a `reconcile_pending_reactions()` startup step scans all non-terminal sessions in reaction-triggering states (e.g., `ci_failed`, `changes_requested`) and re-enqueues reactions for any session where no recent successful delivery entry exists in the action journal. This produces the same effect as re-derivation without re-running a full poll cycle. The action journal's idempotency check prevents duplicate execution on the subsequent tick.

5. **Persisted queue** — Reactions survive crashes without re-derivation. Adds a queue file per session or global queue file, with atomic write semantics matching ADR-0005. Complexity gain is high; benefit is low given that re-derivation is already the crash-recovery strategy for the lifecycle engine itself.

### Wait-for-Ready Implementation

6. **Immediate delivery** — Reactions fire as soon as detected, regardless of agent activity state. Risks delivering a nudge mid-turn, disrupting the agent's reasoning loop. The PRD explicitly rejects this for non-terminal events.

7. **Per-session pending queue with delivery on idle** — Non-terminal reactions are queued per session. Each tick, after gathering activity state, the reaction engine checks whether a queued nudge can be delivered (agent is `ready` or `idle`). Terminal reactions (`tracker-terminal`, `budget_exceeded`) bypass the queue and deliver immediately. Aligns with the PRD's wait-for-ready protocol.

8. **Drop if active** — If the agent is active when a reaction fires, skip it. Simpler but loses the reaction entirely — a CI failure detected while the agent is active would never trigger a fix attempt.

### Scheduling Trigger

9. **Background task with independent timer** — A separate `tokio::spawn` task evaluates dispatch eligibility every N seconds, independent of the poll loop. Creates a second clock; spawn requests must flow through the IPC channel anyway, introducing back-pressure. No benefit over option 10.

10. **Integrated into the poll loop as a fourth phase** — After the existing three phases (gather/evaluate/transition), a react+schedule phase runs. Scheduling evaluates eligible issues from fresh session state (just gathered and transitioned). New sessions are spawned via the same orchestrator-internal path as IPC `Spawn` requests — no channel round-trip required since the scheduler runs inside the poll loop task.

### Priority Evaluation

11. **FIFO with global cap** — Simple priority queue ordered by issue creation time. Ignores issue priority labels. Easy to implement; acceptable for small teams.

12. **Priority-then-age with per-state concurrency limits** — Issues are ranked by tracker priority (highest first), then by creation age (oldest first). Before dispatching, the scheduler checks: (a) global `maxConcurrentAgents` not exceeded, (b) per-state `maxConcurrentAgentsByState` limit not exceeded for the issue's current tracker state, (c) no non-terminal blockers. This matches the PRD exactly and prevents resource starvation where one stage monopolizes all agent slots.

## Decision

**Reaction engine placement:** Option 2 — Separate `ReactionEngine` module. The lifecycle engine enqueues; the reaction engine dequeues.

**Reaction queue durability:** Option 4 — In-memory queue. On startup, `reconcile_pending_reactions()` scans non-terminal sessions and re-enqueues as needed; the action journal prevents duplicate execution.

**Wait-for-ready:** Option 7 — Per-session pending queue, deliver on idle, terminal events bypass.

**Scheduling trigger:** Option 10 — Fourth phase of the poll loop (react+schedule).

**Priority evaluation:** Option 12 — Priority-then-age with per-state limits and blocker exclusion.

The design has five components:

### 1. Reaction Engine

A `ReactionEngine` struct owns: the reaction configuration (from `Config`), the per-session pending queue (`HashMap<SessionId, VecDeque<PendingReaction>>`), and retry counters (`HashMap<(SessionId, ReactionType), RetryState>`).

Project-level reactions use a sentinel key `"{project_id}::project"` in the `pending` HashMap for reactions that are not scoped to a single session (e.g., `all-complete`).

```rust
pub struct ReactionEngine {
    config: Arc<Config>,
    pending: HashMap<SessionId, VecDeque<PendingReaction>>,
    retry_state: HashMap<(SessionId, ReactionType), RetryState>,
}

pub struct PendingReaction {
    pub reaction_type: ReactionType,
    pub payload: ReactionPayload,      // CI log excerpt, review comments, etc.
    pub queued_at: u64,                // Unix milliseconds; wall-clock required for cross-restart consistency
    pub is_terminal: bool,             // bypass wait-for-ready if true
    pub trigger_state: SessionStatus,  // the session status that triggered this reaction; used for stale invalidation
}

pub struct RetryState {
    pub attempts: u32,
    pub next_eligible_at: u64,           // Unix milliseconds; wall-clock required for cross-restart backoff consistency
    pub escalated: bool,
    pub last_delivered_at: Option<u64>,  // Unix milliseconds; used for "once-per-state-entry" spam prevention
}
```

**Enqueue path:** The lifecycle engine's entry actions call `reaction_engine.enqueue(session_id, reaction_type, payload, trigger_state)`. Entry actions remain fire-and-forget — they do not block on delivery. The reaction engine stores the event in `pending[session_id]`.

**Delivery path (per tick, per session):**

1. If `pending[session_id]` is non-empty, check `activity_state` and `session_status` from the just-gathered `PollContext`.
2. **Stale-reaction guard:** Before delivery, verify the session's current `session_status` still matches the pending reaction's `trigger_state`. If the session has transitioned out of the trigger state (e.g., it is now `mergeable` but the reaction is `ci-failed`), discard the stale reaction from the queue and continue to the next queued reaction. Do not deliver and do not append to journal.
3. If `activity_state` is `ready` or `idle` (or reaction `is_terminal = true`): pop the oldest non-stale reaction, check the action journal for deduplication, execute the action, append to journal.
4. If `activity_state` is `active` or `waiting_input`: leave in queue, deliver next tick.
5. After successful execution, check `RetryState.attempts`. If within limits, clear. If the action failed (e.g., `send-to-agent` delivery failed), increment `attempts` and set `next_eligible_at = now + backoff(attempts)`.

**Action types:**

| Action | Implementation |
|--------|----------------|
| `send-to-agent` | `Runtime::execute_step(SendMessage { content })` — identical to `ao send` delivery. Uses the existing busy-detection path. |
| `notify` | `Notifier::notify(priority, message)` — routes via `notificationRouting` from ADR-0003 config. |
| `auto-merge` | `SCM::mergePR(pr_id)` — orchestrator-owned mutation (FR17 preview). Gated by `approved-and-green` reaction config. |

**MVP `ReactionAction` enum:** `SendToAgent`, `Notify`, `AutoMerge`. These three variants are MVP-complete. `ReactionAction::Rework` will be added post-MVP with `require_confirmation: true` defaulting to true.

**10 MVP reaction type defaults:**

| Reaction | Trigger state | Default action | `trigger_version` source | Max retries | Escalation |
|----------|--------------|----------------|--------------------------|-------------|------------|
| `ci-failed` | `ci_failed` | `send-to-agent` (CI log) | CI run ID | 2 | notify urgent after 2 |
| `changes-requested` | `changes_requested` | `send-to-agent` (review comments) | review thread ID | 1 | notify after 30 min |
| `bugbot-comments` | `pr_open` (bot comment detected) | `send-to-agent` (bot feedback) | comment ID | 1 | notify after 30 min |
| `merge-conflicts` | PR merge conflict detected | `send-to-agent` (rebase instructions) | PR head commit SHA (stable proxy for conflict state) | 1 | notify after 15 min |
| `approved-and-green` | `mergeable` | `notify` (or `auto-merge` if enabled) | PR head commit SHA | — | — |
| `agent-stuck` | `stuck` | `notify` urgent | `""` (state-based, once-per-entry) | — | — |
| `agent-needs-input` | `needs_input` | `notify` urgent | `""` (state-based, once-per-entry) | — | — |
| `agent-exited` | `terminated` / runtime dead | `notify` urgent | `""` (state-based, once-per-entry) | — | — |
| `all-complete` | All sessions in project terminal | `notify` summary | `""` (cooldown via journal) | — | — |
| `tracker-terminal` | `cleanup` global edge | `notify` (reaction engine); kill + workspace destroy are lifecycle engine transition side effects, not entry actions | `""` | — | — |

**Post-MVP reaction types (not implemented in MVP):**

| Reaction | Trigger state | Planned action | Notes |
|----------|--------------|----------------|-------|
| `rework-requested` | Issue enters rework tracker state | `Rework` (close PR, fresh branch, re-spawn) | `require_confirmation: true` by default; double-idempotency required |

**Exponential backoff:** `delay = min(base_delay_ms * 2^attempts, config.maxRetryBackoffMs)`. Default `base_delay_ms` = 30s (one poll tick), default `maxRetryBackoffMs` = 300s (5 minutes). Global `maxRetriesPerIssue` (default 5/day) caps total spawn attempts across all reaction-triggered re-spawns for a given issue.

### 2. Idempotency via Action Journal

Before executing any action (send-to-agent, notify, auto-merge), the reaction engine reads the session's action journal and checks for a matching entry within the deduplication window. Dedupe key: `(reaction_type, action_type, target_id, trigger_version)` where `trigger_version` is a change-specific identifier (CI run ID, commit SHA, or review thread ID). Deduplication window: 5 minutes for `send-to-agent` and `auto-merge`; 1 minute for `notify` on *versioned* reactions (those with a concrete `trigger_version`). If a matching entry exists within the window, the action is skipped with result `skipped` (no journal append).

**Notify spam prevention for non-versioned reactions:** State-based reactions that lack a concrete `trigger_version` — `agent-stuck`, `agent-needs-input`, `agent-exited` — use a "once-per-state-entry" deduplication strategy instead of the time window. The `RetryState.last_delivered_at` field records the session's last state-transition timestamp at the time of successful delivery. On the next tick, if the session has not re-entered the trigger state since `last_delivered_at` (i.e., the session's `last_transition_at` timestamp has not advanced), the notification is skipped. This means: one notification fires when the session *enters* the stuck/needs-input/exited state; re-notification only occurs if the session exits and re-enters that state. The `escalation_delay_ms` escalation still applies — if the session remains in the trigger state past the escalation window, the escalation action fires once via the same "once-per-state-entry" gate on the escalation action type.

**`trigger_version` mandate:** Tracker and CI plugins contributing data to `PollContext` MUST provide a unique versioned identifier for all event-based reaction triggers: CI run ID for CI state changes, commit SHA for push events, review thread ID for code review events. This is a contract extension to ADR-0006 (Tracker). For reactions that inherently lack a versioned trigger (state-only reactions: `agent-stuck`, `agent-needs-input`, `agent-exited`), `trigger_version` is set to the empty string `""` and the "once-per-state-entry" spam prevention strategy (described above) applies instead of the time-window dedupe.

The action journal uses JSONL format (one JSON object per line) for typed parsing. After successful execution, append to journal:

```json
{"reaction_type":"ci-failed","action_type":"send-to-agent","target":"session-abc-123","trigger_version":"run_9876543210","timestamp_ms":1741651200000,"dedupe_key":"ci-failed|send-to-agent|session-abc-123|run_9876543210","result":"ok","attempt":1,"source":"reaction_engine"}
```

Failed actions append with `"result":"failed"` and an `"error_code"` field. The journal is the source of truth for retry escalation decisions — `RetryState` is rebuilt from the journal on restart (scanning the session's journal file for recent failed entries of each reaction type).

### 3. Scheduler

The scheduler runs as the final sub-phase of the react+schedule phase, after reaction delivery. It evaluates whether any new sessions should be spawned to handle eligible issues.

**Scheduler inputs (gathered from previous phases):**

- `SessionStore::list()` — all sessions, grouped by project + issue ID
- Active session count per project (from above)
- Active session count per tracker state (from session metadata, using `last_known_tracker_state`)
- `Tracker::get_issue()` results (cached from the gather phase where already fetched; fresh calls for issues with no running session)

**Candidate set:** The scheduler fetches at most the top N issues from the tracker per tick (configurable `schedulerMaxCandidatesPerTick`, default 50) using tracker-native sorting before fine-grained eligibility checks. This bounds API calls regardless of repo size. Note: tracker-native sorting is a best-effort approximation of the configured `priority_rank` mapping. Trackers with native priority fields (Linear, Jira) support true priority-descending sort. Trackers without native priority (GitHub Issues) sort by creation time ascending — the actual dispatch order within the 50-candidate set is still determined by the `priority_rank` mapping, but the candidate set composition itself may not include all high-priority issues if they fall outside the top 50 by creation time. Teams using GitHub Issues with priority labels should set a lower `schedulerMaxCandidatesPerTick` or rely on label-based filtering (if supported by the tracker plugin) to improve candidate quality.

**Dispatch eligibility criteria (all must be true):**

1. Issue is in an `activeStates` tracker state (configurable per project, ADR-0006)
2. No non-terminal session exists for this issue
3. Global `maxConcurrentAgents` not reached
4. Per-state `maxConcurrentAgentsByState` limit not reached for this issue's tracker state
5. No non-terminal direct blocker issues (blocker checking is limited to depth 1 — direct blockers only; tracker-native `is_blocked` flags are used when available)
6. Issue not in the `maxRetriesPerIssue` daily cap — count `spawn` entries in the last 24h across all sessions (including archived) for this issue ID. Since the action journal is per-session (ADR-0005), this requires scanning the journals of all sessions (active and archived) whose metadata carries the same `ISSUE_ID`. The session store's `list()` call returns archived sessions; their journal paths are derived via the same `DataPaths` helper. This cross-session scan is bounded by the number of past sessions for the issue, which is O(`maxRetriesPerIssue`) by definition.

**Dispatch ordering:** All eligible issues are sorted by `(priority_rank ASC, created_at ASC)`. Priority rank is derived from the tracker's priority field — a configurable mapping from tracker priority labels to integer ranks (e.g., `"urgent" → 1, "high" → 2, "medium" → 3, "low" → 4`). Issues with the same priority rank are sorted oldest-first. Issues whose tracker priority label is not present in the configured mapping default to the lowest priority rank (`u32::MAX`), ensuring they are dispatched only after all explicitly-ranked issues. Teams should configure a catch-all mapping entry (`"*" → 5`) if they want custom labels to receive a specific rank.

**Spawn execution:** The scheduler calls `orchestrator.spawn_session(project_id, issue_id)` directly — the same internal method called by the IPC `Spawn` handler. No channel round-trip. Spawns are bounded per tick by `maxSpawnsPerTick` (default: 3) to prevent a burst of eligible issues from overwhelming the system on startup.

**Blocker state:** `SessionMetadata` includes `last_known_tracker_state: Option<String>`, updated during the gather phase. The scheduler reads this field directly rather than making additional tracker API calls during the schedule phase.

### 4. Poll Loop Integration

The updated poll loop phases:

```
tick N:
  ① Drain IPC channel — process Spawn/Kill/Cleanup/Stop requests
  ② Gather — PollContext per active session (runtime, activity, PR/CI, tracker)
  ③ Evaluate — graph walk per session (pure, no I/O)
  ④ Transition — apply state changes, fire entry actions (enqueue reactions)
  ⑤ React — deliver pending reactions (wait-for-ready gated)
  ⑥ Schedule — evaluate eligible issues, spawn new sessions
  ⑦ Sleep until next tick
```

Phases ⑤ and ⑥ are new. They run after transitions are applied so that newly transitioned sessions (e.g., a session just entering `ci_failed`) have their reactions enqueued before phase ⑤ evaluates delivery eligibility.

**`all-complete` detection:** At the end of phase ④, the `all-complete` reaction is enqueued if: (a) the count of non-terminal sessions for the project transitions from ≥1 to 0 this tick (i.e., at least one session entered a terminal state and none remain active), AND (b) at least one session reached a naturally-completed terminal state (`cleanup` or `archived`) this tick — not just `killed`. This prevents `all-complete` from firing when a user manually kills a stuck session with no completed work. The sentinel key `"{project_id}::project"` is used in the `pending` HashMap. A 1-hour cooldown is enforced via the action journal to prevent re-firing as new issues are subsequently added to the project.

### 5. Configuration

The existing `Config` struct (ADR-0003) gains a `reactions` field (already schema'd in FR10) and the existing `maxConcurrentAgents` / `maxConcurrentAgentsByState` / `maxRetryBackoffMs` / `maxRetriesPerIssue` fields are now consumed.

```rust
pub struct ReactionConfig {
    pub enabled: bool,                    // default: true
    pub action: ReactionAction,           // send-to-agent | notify | auto-merge
    pub max_retries: u32,
    pub escalation_delay_ms: Option<u64>,
    pub escalation_action: Option<ReactionAction>,
}

pub enum ReactionAction {
    SendToAgent { template: Option<String> },  // None = default template per reaction type
    Notify { priority: NotificationPriority },
    AutoMerge,
    // Rework variant is post-MVP (require_confirmation: true by default)
}
```

Per-project `reactions` overrides merge with global `reactions`. Each field in `ReactionConfig` falls back independently to the corresponding global value if absent from the project config; absent global fields fall back to struct defaults. Project-level keys win for any field that is explicitly set. This allows a project to disable a reaction type (`enabled: false`) or lower its retry count without affecting other projects.

Hot-reload (ADR-0003) applies: changes to reaction config and concurrency limits take effect on the next poll tick without restart. In-flight retry state is preserved across hot-reloads — `RetryState` is keyed by session ID and reaction type, not by config values.

## Consequences

Positive:

- Separating the reaction engine from the lifecycle engine preserves ADR-0001's core invariant: the graph walk is pure, no I/O in evaluation. Entry actions remain fire-and-forget stubs; the reaction engine owns all delivery and retry complexity.
- The wait-for-ready protocol prevents reaction delivery from disrupting agent mid-turn reasoning. The pending queue is a natural buffer; no inter-process signaling required.
- Crash recovery is handled by `reconcile_pending_reactions()` at startup: it scans all non-terminal sessions in reaction-triggering states and re-enqueues reactions for any session where no recent successful delivery entry exists in the action journal. The action journal's idempotency check (keyed on `(reaction_type, action_type, target_id, trigger_version)`) prevents duplicate execution. No separate recovery codepath beyond this startup scan is needed.
- Integrating scheduling into the poll loop (phase ⑥) means scheduling always operates on freshly gathered and transitioned state. No stale data, no clock synchronization. The scheduler inherits the re-entrancy guard from the poll loop.
- Priority-then-age dispatch with per-state limits directly matches the PRD spec. The implementation is a sort + filter on each tick — no persistent priority queue to maintain. Candidate set is bounded to `schedulerMaxCandidatesPerTick` (default 50) issues per tick using tracker-native sorting, so scheduling complexity is O(min(active tracker issues, schedulerMaxCandidatesPerTick)) regardless of repo size.
- Per-project reaction overrides and global defaults give teams fine-grained control without requiring code changes. The `enabled: false` escape hatch lets teams opt out of specific automations.

Negative:

- Adding two new phases to the poll loop (react and schedule) increases tick duration. With 50+ sessions and slow tracker API calls in the gather phase, ticks already risk exceeding 30s (re-entrancy guard will skip). Reaction delivery (send-to-agent) involves additional tmux interaction. Mitigation: reaction delivery and scheduling are bounded — maximum one reaction delivered per session per tick, maximum `maxSpawnsPerTick` new sessions per tick. Scheduling is O(min(active tracker issues, schedulerMaxCandidatesPerTick)) not O(all issues in the repo.
- The `all-complete` detection requires checking all sessions across a project in phase ④ — O(sessions per project). This is already done in phase ③ (graph evaluation iterates all sessions), so no additional I/O is needed.
- Retry state (`RetryState`) is rebuilt from the action journal on restart. Journal scans are O(journal entries per session). For long-running sessions with many reactions, this scan grows. Mitigation: entries older than `maxRetryBackoffMs` × 2 are irrelevant to current retry state and can be skipped during scan. A future compaction step could truncate old journal entries.
- Per-state concurrency limits (`maxConcurrentAgentsByState`) require knowing the current tracker state of each active session. `SessionMetadata` includes `last_known_tracker_state: Option<String>`, updated during the gather phase. The scheduler reads this field directly to avoid extra tracker API calls in the schedule phase.
- The `rework-requested` reaction (close PR, fresh branch, re-spawn) is a post-MVP stub. When implemented, it must check the action journal for a recent `rework` entry with the same issue ID before proceeding — double-idempotency protection — because a rework mid-flight followed by an orchestrator crash would otherwise trigger a second rework on restart. `ReactionAction::Rework` will be added post-MVP with `require_confirmation: true` defaulting to true.

Reference `docs/plans/` for implementation pseudocode and module structure.

---

## Council Review Findings Addressed (Round 2)

| Finding | Description | Resolution |
|---------|-------------|------------|
| CF-1 (HIGH) | Stale pending queue — reactions delivered after triggering condition resolves | Added `trigger_state: SessionStatus` field to `PendingReaction`; added stale-reaction guard in delivery path (step 2): verify session's current status still matches `trigger_state` before delivering; discard if mismatched |
| CF-2 (MEDIUM) | Notify spam for state-based non-versioned reactions (`stuck`, `needs_input`, `exited`) | Added "once-per-state-entry" deduplication strategy using `RetryState.last_delivered_at`; extended `RetryState` struct with the field; updated idempotency section |
| CC MEDIUM | `all-complete` fires on `killed` sessions with no completed work | Refined `all-complete` trigger: requires at least one session reaching a naturally-completed terminal state (`cleanup`/`archived`) this tick, not just any terminal state; also requires non-terminal count to transition from ≥1 to 0 |
| Gemini MEDIUM | `trigger_version` availability not mandated from tracker plugins | Added explicit `trigger_version` mandate paragraph in Component 2; specifies empty string `""` for state-only reactions with "once-per-state-entry" fallback |
| CC/Gemini LOW | Unmapped priority labels have no default fallback | Added `u32::MAX` as default fallback for unmapped labels; documented catch-all `"*"` mapping option |
| CC HIGH (late) | `tracker-terminal` entry action incorrectly described as owning kill+destroy (violates ADR-0001 pure-graph-walk) | Fixed table cell: kill+destroy are lifecycle engine transition side effects; reaction engine sends `notify` only |
| CC MEDIUM (late) | `trigger_version` sourcing not enumerated per reaction type | Added `trigger_version` source column to MVP reaction table with per-type values |
| CC MEDIUM (late) | `maxRetriesPerIssue` cap scan is per-session; cross-session aggregation unspecified | Criterion 6 now specifies cross-session scan of all sessions sharing `ISSUE_ID` (active + archived) |
| CC MEDIUM (late) | Tracker-native sorting may not align with `priority_rank` mapping (GitHub lacks native priority) | Documented as best-effort approximation with per-tracker guidance |
| CC LOW (late) | `rework-requested` indistinguishable from MVP reactions in table | Separated into MVP table (10 rows, renamed) and post-MVP table; `rework-requested` moved to post-MVP |

## Council Review Findings Addressed (Round 1)

| Finding | Description | Resolution |
|---------|-------------|------------|
| HIGH-2 | Crash-recovery claim in Option 4 was incorrect ("entry actions re-fire") | Replaced with `reconcile_pending_reactions()` startup step that scans non-terminal sessions and re-enqueues where no recent successful journal entry exists |
| CF-1 / HIGH-1 | `rework-requested` was included in MVP `ReactionAction` enum | Marked as post-MVP stub in reaction table; removed from MVP enum; noted `ReactionAction::Rework` post-MVP with `require_confirmation: true` |
| CF-2 | Scheduler O-complexity was incorrectly stated as O(active sessions) | Corrected to O(min(active tracker issues, schedulerMaxCandidatesPerTick)); added `schedulerMaxCandidatesPerTick` config (default 50) with tracker-native sorting to bound API calls |
| CF-3 | Dedupe key was `(action_type, target_id)` — insufficient | Changed to `(reaction_type, action_type, target_id, trigger_version)` where `trigger_version` is CI run ID, commit SHA, or review thread ID; updated journal format |
| MEDIUM-3 | `last_known_tracker_state` was hedged as "should cache" | Made a definitive decision: `last_known_tracker_state: Option<String>` is a `SessionMetadata` field, updated in the gather phase; referenced consistently in Component 3 and Consequences |
| MEDIUM-4 | Config override merge semantics were underspecified | Added field-level inheritance: each `ReactionConfig` field falls back independently to global value, then struct default |
| LOW-1 | `all-complete` queue key was unspecified | Specified sentinel key `"{project_id}::project"` in `pending` HashMap for project-level reactions |
| LOW-2 | `queued_at: Instant` not cross-restart safe | Changed to `queued_at: u64` (Unix milliseconds) with comment explaining wall-clock requirement |
| Gemini: Blocker depth | Blocker checking depth was unspecified | Added: depth 1 (direct blockers only); tracker-native `is_blocked` flags used when available |
| Gemini: `all-complete` cooldown | No cooldown on `all-complete` re-firing | Added 1-hour cooldown enforced via action journal to prevent re-firing as new issues are added |
| Gemini: Action journal format | Journal was pipe-delimited text | Changed to JSONL format (one JSON object per line); updated journal entry example to JSON |
