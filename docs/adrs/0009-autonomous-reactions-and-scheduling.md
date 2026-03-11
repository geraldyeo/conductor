# ADR-0009: Autonomous Reactions and Scheduling

## Status
Accepted

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

4. **In-memory queue** — Reactions are lost on orchestrator crash. However, reactions are re-derivable: on restart, the poll loop re-evaluates all non-terminal sessions, entry actions re-fire for sessions already in reaction-triggering states (e.g., `ci_failed`, `changes_requested`), and the action journal's idempotency check prevents duplicate execution. Crash recovery is identical to normal tick re-evaluation — no separate recovery codepath.

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

**Reaction queue durability:** Option 4 — In-memory queue. Re-derivation via re-poll is the existing crash recovery strategy; no second mechanism needed.

**Wait-for-ready:** Option 7 — Per-session pending queue, deliver on idle, terminal events bypass.

**Scheduling trigger:** Option 10 — Fourth phase of the poll loop (react+schedule).

**Priority evaluation:** Option 12 — Priority-then-age with per-state limits and blocker exclusion.

The design has five components:

### 1. Reaction Engine

A `ReactionEngine` struct owns: the reaction configuration (from `Config`), the per-session pending queue (`HashMap<SessionId, VecDeque<PendingReaction>>`), and retry counters (`HashMap<(SessionId, ReactionType), RetryState>`).

```rust
pub struct ReactionEngine {
    config: Arc<Config>,
    pending: HashMap<SessionId, VecDeque<PendingReaction>>,
    retry_state: HashMap<(SessionId, ReactionType), RetryState>,
}

pub struct PendingReaction {
    pub reaction_type: ReactionType,
    pub payload: ReactionPayload,   // CI log excerpt, review comments, etc.
    pub queued_at: Instant,
    pub is_terminal: bool,          // bypass wait-for-ready if true
}

pub struct RetryState {
    pub attempts: u32,
    pub next_eligible_at: Instant,
    pub escalated: bool,
}
```

**Enqueue path:** The lifecycle engine's entry actions call `reaction_engine.enqueue(session_id, reaction_type, payload)`. Entry actions remain fire-and-forget — they do not block on delivery. The reaction engine stores the event in `pending[session_id]`.

**Delivery path (per tick, per session):**

1. If `pending[session_id]` is non-empty, check `activity_state` from the just-gathered `PollContext`.
2. If `activity_state` is `ready` or `idle` (or reaction `is_terminal = true`): pop the oldest pending reaction, check the action journal for deduplication, execute the action, append to journal.
3. If `activity_state` is `active` or `waiting_input`: leave in queue, deliver next tick.
4. After successful execution, check `RetryState.attempts`. If within limits, clear. If the action failed (e.g., `send-to-agent` delivery failed), increment `attempts` and set `next_eligible_at = now + backoff(attempts)`.

**Action types:**

| Action | Implementation |
|--------|----------------|
| `send-to-agent` | `Runtime::execute_step(SendMessage { content })` — identical to `ao send` delivery. Uses the existing busy-detection path. |
| `notify` | `Notifier::notify(priority, message)` — routes via `notificationRouting` from ADR-0003 config. |
| `auto-merge` | `SCM::mergePR(pr_id)` — orchestrator-owned mutation (FR17 preview). Gated by `approved-and-green` reaction config. |

**11 Reaction type defaults:**

| Reaction | Trigger state | Default action | Max retries | Escalation |
|----------|--------------|----------------|-------------|------------|
| `ci-failed` | `ci_failed` | `send-to-agent` (CI log) | 2 | notify urgent after 2 |
| `changes-requested` | `changes_requested` | `send-to-agent` (review comments) | 1 | notify after 30 min |
| `bugbot-comments` | `pr_open` (bot comment detected) | `send-to-agent` (bot feedback) | 1 | notify after 30 min |
| `merge-conflicts` | PR merge conflict detected | `send-to-agent` (rebase instructions) | 1 | notify after 15 min |
| `approved-and-green` | `mergeable` | `notify` action (or `auto-merge` if enabled) | — | — |
| `agent-stuck` | `stuck` | `notify` urgent | — | — |
| `agent-needs-input` | `needs_input` | `notify` urgent | — | — |
| `agent-exited` | `terminated` / runtime dead | `notify` urgent | — | — |
| `all-complete` | All sessions in project terminal | `notify` summary | — | — |
| `tracker-terminal` | `cleanup` global edge | `kill` + workspace destroy (entry action handles; reaction engine sends `notify`) | — | — |
| `rework-requested` | Issue enters rework tracker state | Close PR, fresh branch from default, re-spawn | — | notify if re-spawn fails |

**Exponential backoff:** `delay = min(base_delay_ms * 2^attempts, config.maxRetryBackoffMs)`. Default `base_delay_ms` = 30s (one poll tick), default `maxRetryBackoffMs` = 300s (5 minutes). Global `maxRetriesPerIssue` (default 5/day) caps total spawn attempts across all reaction-triggered re-spawns for a given issue.

### 2. Idempotency via Action Journal

Before executing any action (send-to-agent, notify, auto-merge), the reaction engine reads the session's action journal and checks for a matching entry within the deduplication window. Dedupe key: `(action_type, target_id)`. Deduplication window: 5 minutes for `send-to-agent` and `auto-merge`; 1 minute for `notify` (notifications are lower risk to repeat). If a matching entry exists within the window, the action is skipped with result `skipped` (no journal append).

After successful execution, append to journal:

```
{action_type}|{target}|{timestamp_ms}|{dedupe_key}|{result}|{attempt}|reaction_engine
```

Failed actions append with `result=failed` and `error_code`. The journal is the source of truth for retry escalation decisions — `RetryState` is rebuilt from the journal on restart (scanning the session's journal file for recent failed entries of each reaction type).

### 3. Scheduler

The scheduler runs as the final sub-phase of the react+schedule phase, after reaction delivery. It evaluates whether any new sessions should be spawned to handle eligible issues.

**Scheduler inputs (gathered from previous phases):**

- `SessionStore::list()` — all sessions, grouped by project + issue ID
- Active session count per project (from above)
- Active session count per tracker state (from session metadata)
- `Tracker::get_issue()` results (cached from the gather phase where already fetched; fresh calls for issues with no running session)

**Dispatch eligibility criteria (all must be true):**

1. Issue is in an `activeStates` tracker state (configurable per project, ADR-0006)
2. No non-terminal session exists for this issue
3. Global `maxConcurrentAgents` not reached
4. Per-state `maxConcurrentAgentsByState` limit not reached for this issue's tracker state
5. No non-terminal blocker issues (blockers are checked via `Tracker::get_issue()` on each blocker ID)
6. Issue not in the `maxRetriesPerIssue` daily cap (checked via action journal — count `spawn` entries in the last 24h for this issue)

**Dispatch ordering:** All eligible issues are sorted by `(priority_rank ASC, created_at ASC)`. Priority rank is derived from the tracker's priority field — a configurable mapping from tracker priority labels to integer ranks (e.g., `"urgent" → 1, "high" → 2, "medium" → 3, "low" → 4`). Issues with the same priority rank are sorted oldest-first.

**Spawn execution:** The scheduler calls `orchestrator.spawn_session(project_id, issue_id)` directly — the same internal method called by the IPC `Spawn` handler. No channel round-trip. Spawns are bounded per tick by `maxSpawnsPerTick` (default: 3) to prevent a burst of eligible issues from overwhelming the system on startup.

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

**`all-complete` detection:** At the end of phase ④, if all sessions for a project are in terminal states and at least one transitioned to terminal this tick, the `all-complete` reaction is enqueued for the project.

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
}
```

Per-project `reactions` overrides merge with global `reactions`. Project-level keys win. This allows a project to disable a reaction type (`enabled: false`) or lower its retry count without affecting other projects.

Hot-reload (ADR-0003) applies: changes to reaction config and concurrency limits take effect on the next poll tick without restart. In-flight retry state is preserved across hot-reloads — `RetryState` is keyed by session ID and reaction type, not by config values.

## Consequences

Positive:

- Separating the reaction engine from the lifecycle engine preserves ADR-0001's core invariant: the graph walk is pure, no I/O in evaluation. Entry actions remain fire-and-forget stubs; the reaction engine owns all delivery and retry complexity.
- The wait-for-ready protocol prevents reaction delivery from disrupting agent mid-turn reasoning. The pending queue is a natural buffer; no inter-process signaling required.
- Crash recovery is zero-cost: the reaction engine's in-memory queue is re-populated on restart via the same entry action re-firing that the lifecycle engine already uses for crash recovery (re-poll → re-evaluate → entry actions fire → reactions enqueue). The action journal's idempotency check prevents duplicate execution.
- Integrating scheduling into the poll loop (phase ⑥) means scheduling always operates on freshly gathered and transitioned state. No stale data, no clock synchronization. The scheduler inherits the re-entrancy guard from the poll loop.
- Priority-then-age dispatch with per-state limits directly matches the PRD spec. The implementation is a sort + filter on each tick — no persistent priority queue to maintain.
- Per-project reaction overrides and global defaults give teams fine-grained control without requiring code changes. The `enabled: false` escape hatch lets teams opt out of specific automations.

Negative:

- Adding two new phases to the poll loop (react and schedule) increases tick duration. With 50+ sessions and slow tracker API calls in the gather phase, ticks already risk exceeding 30s (re-entrancy guard will skip). Reaction delivery (send-to-agent) involves additional tmux interaction. Mitigation: reaction delivery and scheduling are bounded — maximum one reaction delivered per session per tick, maximum `maxSpawnsPerTick` new sessions per tick. Both phases are O(active sessions) not O(all issues).
- The `all-complete` detection requires checking all sessions across a project in phase ④ — O(sessions per project). This is already done in phase ③ (graph evaluation iterates all sessions), so no additional I/O is needed.
- Retry state (`RetryState`) is rebuilt from the action journal on restart. Journal scans are O(journal entries per session). For long-running sessions with many reactions, this scan grows. Mitigation: entries older than `maxRetryBackoffMs` × 2 are irrelevant to current retry state and can be skipped during scan. A future compaction step could truncate old journal entries.
- Per-state concurrency limits (`maxConcurrentAgentsByState`) require knowing the current tracker state of each active session. `SessionMetadata` does not currently store the issue's tracker state — the scheduler would need to fetch it via `Tracker::get_issue()` for all active sessions on each tick, or cache it in session metadata after each gather. The implementation should cache `last_known_tracker_state` in session metadata (updated during the gather phase) to avoid extra tracker API calls in the schedule phase.
- The `rework-requested` reaction (close PR, fresh branch, re-spawn) is the most destructive automated action. It must check the action journal for a recent `rework` entry with the same issue ID before proceeding — double-idempotency protection — because a rework mid-flight (PR closing, branch creation) followed by an orchestrator crash would otherwise trigger a second rework on restart.

Reference `docs/plans/` for implementation pseudocode and module structure.
