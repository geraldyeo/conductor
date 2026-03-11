# ADR-0010: Orchestrator-as-Session

## Status
Accepted (revised — round 2 findings addressed)

## Context

`ao start` currently starts a foreground poll loop and IPC listener but manages sessions purely via rule-based transitions and (post ADR-0009) declarative reactions. FR13 adds a special orchestrator session — an AI agent that runs alongside the poll loop and can autonomously coordinate work: spawning sessions for new issues, monitoring status, sending nudges, and making judgment calls that rules cannot easily encode.

The ADR-0007 deferred item explicitly names this as the highest-priority post-MVP feature and lists its two unmet dependencies: ADR-0008 (Prompt System, now landed) and FR5 (Scheduling, covered by ADR-0009). Both are now resolved.

The orchestrator-as-session enables a fully autonomous operating mode: a human starts `ao start`, the orchestrator AI agent takes over coordination, and the human is only involved for approval-gated actions (merges, major interventions).

Key forces:

- The orchestrator agent needs a human-attachable terminal (for debugging and manual intervention). This rules out in-process async tasks.
- The orchestrator agent's session must be monitored for liveness — it should auto-restart if it crashes, since the system degrades without it. This requires the lifecycle engine's liveness checks, but with different rules than worker sessions (no tracker issue, no PR/CI state to poll).
- The orchestrator agent acts by issuing `ao` commands. These are already designed to be machine-readable (`--json` global flag) and are scriptable by construction. ADR-0007's IPC control plane is the execution path.
- The orchestrator agent's prompt is already scaffolded: ADR-0008 defines `PromptEngine::render_orchestrator()`, a distinct render path from the worker `render_launch()`. The orchestrator prompt contains the full `ao` command reference, session management workflows, and common automation patterns.
- The orchestrator agent does not work on a tracker issue. Its session ID cannot follow the `{prefix}-{issueId}-{attempt}` format (ADR-0005). It needs a distinct identity that the lifecycle engine and session store can recognize.
- FR17 (Mutation Authority) is deferred, but this ADR establishes the conceptual split: the orchestrator agent has authority to invoke all `ao` commands including lifecycle mutations (spawn, kill, merge), while worker sessions are expected to request these via comments/workpad rather than direct invocation. Mechanical enforcement is FR17's responsibility.

Five prior ADRs constrain the design:

1. **ADR-0001** — The lifecycle engine's poll loop and graph evaluation apply to the orchestrator session, but with a stripped-down gather phase (no tracker/PR/CI). Entry actions for orchestrator sessions do not enqueue reactions (ADR-0009) — the orchestrator agent itself decides how to react.
2. **ADR-0004** — The orchestrator session uses the same `Agent` and `Runtime` plugin stack as worker sessions. The `LaunchPlan` produced by `Agent::launch_plan()` is the spawn mechanism. For the orchestrator, the agent plugin is always `claude-code` (or configurable); the runtime is `tmux`.
3. **ADR-0005** — `SessionStore` stores the orchestrator session with ID `{prefix}-orchestrator`. The orchestrator session's metadata lives in `{orchestrator_root}/sessions/{prefix}-orchestrator/` using the global orchestrator root (`~/.agent-orchestrator/`), not a per-project FNV-1a hashed path. `DataPaths` gains an `orchestrator_root()` accessor returning the base path without a project hash. The orchestrator session has `IS_ORCHESTRATOR=true` in metadata (canonical uppercase, matching the `KEY=VALUE` storage convention from ADR-0005).
4. **ADR-0007** — The orchestrator agent invokes `ao` commands via shell execution. The IPC control plane (`ao spawn`, `ao send`, `ao session kill`, etc.) processes these commands identically to human-issued commands. The orchestrator session name is recognized so that `ao send` and `ao session ls` can label it distinctly.
5. **ADR-0008** — `PromptEngine::render_orchestrator()` generates the orchestrator system prompt. This is called once at session spawn. The prompt includes: `ao` command reference, session management workflows, available notifier channels, and current project list. It does not include issue-specific content.

## Considered Options

### Process Model

1. **In-process async task** — The orchestrator agent runs as a `tokio::spawn` task inside the orchestrator daemon. Direct access to all state; no IPC round-trip for commands. But: no human-attachable terminal, no ability to use Claude Code's existing tool set (filesystem, bash, etc.) from within an async task, difficult to debug. Fundamentally different from worker sessions — requires a separate code path for "AI coordination."

2. **Reuse existing Agent + Runtime plugin stack (tmux session)** — The orchestrator agent is spawned as a tmux session using the same `LaunchPlan` mechanism as worker sessions. It runs as a real Claude Code process in a tmux window, can use all its tools, and is human-attachable. The lifecycle engine monitors it (simplified gather). The only differences are: special session ID, no tracker issue, no PR/CI gathering, auto-restart on death. Reuses all existing infrastructure.

3. **Separate long-lived process with its own IPC** — The orchestrator agent is a separate binary or daemon that communicates with the lifecycle engine via a second socket. Maximum isolation, but: doubles process management complexity, adds a new IPC protocol, and provides no user-visible benefit over option 2 (the tmux session already provides isolation).

### `ao` Command Invocation from Orchestrator Agent

4. **Shell execution** — Claude Code's bash tool runs `ao spawn --json ...`, `ao session ls --json`, etc. Commands are already scriptable. No new infrastructure. The orchestrator agent sees structured JSON output and acts on it.

5. **Structured MCP tool calls** — The orchestrator provides `ao` commands as MCP tool definitions. The agent calls tools rather than issuing shell commands. Cleaner protocol, better error handling, no shell escaping issues. Requires defining MCP tool schemas for each `ao` command and a tool executor in the orchestrator daemon.

6. **Read-only observation only** — The orchestrator agent only reads `ao status --json` output and posts suggestions as issue comments. Humans execute suggested actions. Defeats the FR13 goal of autonomous coordination.

### Lifecycle on Orchestrator Agent Death

7. **Auto-restart with backoff** — If the orchestrator agent's runtime is detected as not alive, the orchestrator schedules a restart after a short delay (default: 30s, configurable via `orchestratorRestartDelayMs`). This is a special lifecycle path: the session transitions to `errored` in metadata, a restart is queued, and on the next eligible tick a new orchestrator session is spawned (same ID, incremented attempt counter in metadata). The old session is archived.

8. **One-shot: notify on death** — If the orchestrator agent dies, log a warning and notify the human. The human restarts `ao start`. Simpler implementation but creates a gap in autonomous operation.

9. **Infinite restart loop** — Always restart immediately on death. Risk: crash loops consume resources. No circuit breaker.

### Session Identity

10. **Fixed ID `{prefix}-orchestrator`** — Single well-known session ID. Simple lookup. The lifecycle engine and CLI recognize it by suffix. Allows exactly one orchestrator session per orchestrator instance.

11. **Issue-style ID with synthetic issue ID** — Force the orchestrator session into the existing session ID format with a synthetic issue ID (e.g., issue `_orchestrator`). Reuses all existing session store logic without special-casing, but is a hack — `Tracker::get_issue("_orchestrator")` would need to be short-circuited everywhere.

## Decision

**Process model:** Option 2 — Reuse existing Agent + Runtime plugin stack. The orchestrator session is a real Claude Code process in a tmux window, managed by the lifecycle engine with simplified gather rules.

**`ao` command invocation:** Option 4 — Shell execution for the MVP of this feature. Claude Code's bash tool runs `ao` commands with `--json`. Option 5 (MCP tools) is listed as a planned post-MVP enhancement — it would enable better error handling and structured round-trips, but adds scaffolding complexity. The shell path is immediately functional. (This falls under FR17's mechanical enforcement scope — structured tool definitions provide stronger type safety than shell invocation and align with the mutation authority model.)

**Lifecycle on death:** Option 7 — Auto-restart with configurable backoff and a circuit breaker (max 3 restarts per hour, configurable). After the circuit breaker trips, notify the human and stop restarting.

**Session identity:** Option 10 — Fixed ID `{prefix}-orchestrator`. Simple, unambiguous, no synthetic hacks.

The design has four components:

### 1. Session Identity and Metadata

The orchestrator session has ID `{prefix}-orchestrator`. The `{prefix}` is derived from the config's `sessionPrefix` field (same derivation as worker sessions — first project in config or a global prefix). If multiple projects share one orchestrator, the prefix is the orchestrator-level prefix (`orchestratorSessionPrefix` config field, defaulting to the lexicographically first project prefix (by project ID, case-insensitive)). If multiple projects exist and `orchestratorSessionPrefix` is not set, the lexicographic default ensures stable IDs regardless of config file ordering. Teams with multiple projects are encouraged to set `orchestratorSessionPrefix` explicitly.

Session metadata includes a special field `IS_ORCHESTRATOR=true` that the lifecycle engine, session store, and CLI use to apply orchestrator-specific behavior:

- `ao session ls` labels the orchestrator session with `[orchestrator]` rather than an issue ID.
- `ao status` pins the orchestrator session row at the top of the output.
- The lifecycle engine applies simplified gather and recovery rules (see component 3).

The orchestrator session does not have `ISSUE_ID`, `BRANCH`, or `PR_NUMBER` metadata fields. It has `RESTART_TIMESTAMPS` (a list of up to `maxOrchestratorRestarts + 1` Unix millisecond timestamps, appended on each restart — used by the sliding-window circuit breaker). `LAST_RESTART_AT` is removed: it was redundant with `max(RESTART_TIMESTAMPS)` and introduced a split-write risk where an interrupted metadata write could leave the two fields inconsistent. The `ao session ls` display uses `max(RESTART_TIMESTAMPS)` directly for the "last restarted at" column.

### 2. Spawn Sequence

The orchestrator session is spawned during `ao start` startup, after the poll loop and IPC listener are ready (step 8 of the ADR-0007 startup sequence becomes step 8a). This ordering ensures the orchestrator agent can immediately issue `ao` commands against a running orchestrator daemon.

Spawn sequence for the orchestrator session:

1. Check if a session with ID `{prefix}-orchestrator` exists and is non-terminal. For orchestrator sessions, `errored` counts as non-terminal (pending restart). If found and in `errored` state, skip to restart path. If found and in any other non-terminal state, skip spawn entirely.
2. Derive `workspace_path` — create a dedicated worktree at `{orchestrator_root}/orchestrator-workspace/` where `{orchestrator_root}` is the global orchestrator data root (`~/.agent-orchestrator/`, not a per-project FNV-1a hashed path). The orchestrator session's metadata likewise lives at `{orchestrator_root}/sessions/{prefix}-orchestrator/` — outside the per-project hash directories used by worker sessions (`DataPaths::new()`). This global root is used because the orchestrator session spans all projects; no single project's hashed path is canonical. The worktree is checked out to the default branch of the *primary project's repository* (the project whose ID provides the `orchestratorSessionPrefix`; defaults to the lexicographically first project ID). In multi-project/multi-repo setups, this worktree tracks one repository; the orchestrator agent reads other project repositories via absolute paths or by calling `ao status --json` for live state. The orchestrator does not need to modify code via this worktree.
3. Call `PromptEngine::render_orchestrator(config, projects, session_snapshot)` — generates the system prompt.
4. Construct `LaunchPlan` via `Agent::launch_plan(LaunchParams { session_id, workspace_path, prompt, delivery: PromptDelivery::PostLaunch })`. (PostLaunch is required because the claude-code agent plugin starts in interactive mode; the system prompt cannot be passed as a CLI argument — same constraint as worker sessions per ADR-0004.)
5. Execute steps via `Runtime::execute_step()` — create tmux session, launch agent, deliver prompt.
6. Write session metadata (`IS_ORCHESTRATOR=true`, `STATUS=spawning`).
7. The poll loop's next tick detects the running agent and transitions to `working` (simplified evaluate path).

The orchestrator session uses `promptDelivery: "post-launch"` (same as claude-code workers) — the prompt is sent after the agent starts in interactive mode.

### 3. Lifecycle Engine Integration

The lifecycle engine detects `IS_ORCHESTRATOR=true` in session metadata and applies a stripped-down evaluation path:

**Gather phase (orchestrator session):**
- Check runtime liveness (`Runtime::is_alive()`) — same as workers.
- Check activity state (`Agent::detect_activity()`) — same as workers (for human visibility in `ao status`).
- Skip: tracker state, PR detection, CI status, review decision, mergeable check. None are applicable.

**Evaluate phase (orchestrator session):**
The following transitions apply. Note that `errored` is NON-TERMINAL for orchestrator sessions (unlike worker sessions where `errored` is terminal) — the auto-restart mechanism uses `errored` as a transient state pending the next restart attempt.

The precedence values below are orchestrator-local — they are evaluated in an isolated context separate from ADR-0001's global band namespace (bands 0–2 for global kill/budget, 3–27 for local worker edges). The orchestrator evaluate path is a distinct code branch (`IS_ORCHESTRATOR=true` gate), so these local precedence values do not conflict with or shadow the global bands.

| From | To | Trigger | Local Precedence |
|------|----|---------|------------|
| `any` | `killed` | Manual `ao session kill {prefix}-orchestrator` | 0 |
| `spawning` | `working` | Agent process detected as active | 1 |
| `working` | `errored` | Runtime not alive | 2 |
| `errored` | `spawning` | Restart scheduled and circuit breaker not tripped | 3 |
| `working` | `errored` | `detect_activity()` reports `idle` continuously for `orchestratorActivityTimeoutMs` (default: 3600000ms = 1 hour) | 4 |

The two `working→errored` rules (precedence 2 and 4) share the same destination. Evaluation short-circuits after the first matching rule per session tick — if the runtime is not alive (precedence 2), the activity-timeout rule (precedence 4) is not evaluated. Both routes lead to the same `errored` entry action regardless.

The last transition (hung orchestrator detection) uses a much longer threshold than worker stuck detection and routes through auto-restart rather than human escalation.

No `stuck` detection (the orchestrator agent may be idle between coordination tasks). No `ci_failed`, `pr_open`, `changes_requested`, or other PR-lifecycle states.

**Transition phase (orchestrator session on `errored`):**
Entry action for `errored` on an orchestrator session: instead of notifying a human and stopping (as for worker sessions), schedule an auto-restart:

1. Read `RESTART_TIMESTAMPS` from metadata.
2. Check circuit breaker (sliding-window algorithm): count entries in `RESTART_TIMESTAMPS` where `timestamp > now - orchestratorRestartWindowMs`. If count >= `maxOrchestratorRestarts` (default: 3), trip the breaker — notify human, do not restart. This is a sliding window — there is no fixed reset time.
3. Check whether a daemon shutdown is in progress (read the `shutdown_tx` signal from ADR-0007). If shutdown is pending, skip the restart and log "restart suppressed: daemon shutting down." This prevents a restart race where the orchestrator transitions to `errored` just as `ao stop` executes.
4. Otherwise: append the current timestamp to `RESTART_TIMESTAMPS` (capped at `maxOrchestratorRestarts + 1` entries), then queue a restart after `orchestratorRestartDelayMs` (default: 30000ms = 30s). The restart executes as a scheduled task within the poll loop at the next eligible tick, transitioning the session back to `spawning` and re-running the spawn sequence (component 2, step 1 recognizes `errored` as non-terminal and proceeds to the restart path).

**Startup reconciliation (cold-start recovery for `errored` orchestrator):**
On daemon startup, if an orchestrator session is found in `errored` state in SessionStore:

1. Read `RESTART_TIMESTAMPS` and `LAST_RESTART_AT` from metadata.
2. Evaluate the circuit breaker using the sliding-window algorithm (count restarts within `orchestratorRestartWindowMs`).
3. If the circuit breaker is not tripped, re-enqueue the restart after `orchestratorRestartDelayMs`.
4. If tripped, notify the human and leave the session in `errored`.

This makes restart logic crash-safe without a separate in-memory queue — the restart state is fully re-derivable from SessionStore on startup.

**IPC handler guards:**

- **Reserved identity guard:** The IPC `Spawn` and `BatchSpawn` handlers reject any request where: (a) the supplied session ID matches the `*-orchestrator` pattern, or (b) the supplied metadata contains `IS_ORCHESTRATOR=true`. The rejection error message is: "reserved session identity — orchestrator sessions may only be spawned by the daemon lifecycle." This prevents any agent (including a compromised worker) from spawning a clone orchestrator that bypasses the single-instance identity rule enforced by the daemon's own startup path.
- **Spawn rate limit:** The IPC handler enforces `orchestratorSpawnRateLimitPerMinute` (default: 5) for `Spawn` and `BatchSpawn` requests. `BatchSpawn` requests are counted as N individual tokens — a batch of 20 sessions consumes 20 rate-limit tokens. If accepting the full batch would exceed the limit, the entire batch is rejected (not partially accepted); the orchestrator agent must split large batches or retry after the sliding window advances. The rate limit is tracked per-orchestrator-instance in memory. This is distinct from `maxConcurrentAgents` (a concurrency cap) — this is a rate cap preventing burst-then-complete patterns that could overwhelm the tracker or SCM APIs.
- **Worker-on-orchestrator guard:** The IPC handler checks that `Kill` and `Stop` requests targeting `{prefix}-orchestrator` are rejected when the requesting process is identified as a non-orchestrator worker session. In practice, `ao session kill` passes the calling process's session context (read from the `AO_SESSION` environment variable); if `AO_SESSION` identifies a worker session, the IPC handler returns a permission error. This is a soft ergonomic guard providing protection against accidental worker-initiated kills — it does not provide a hard authorization boundary since `AO_SESSION` is caller-controlled. FR17's scoped credential enforcement is the full solution.

**New config fields introduced by this component:**
- `orchestratorRestartDelayMs` (default: 30000)
- `orchestratorRestartWindowMs` (default: 3600000)
- `maxOrchestratorRestarts` (default: 3)
- `orchestratorActivityTimeoutMs` (default: 3600000)
- `orchestratorSpawnRateLimitPerMinute` (default: 5)

### 4. Orchestrator Prompt

`PromptEngine::render_orchestrator()` composes the orchestrator system prompt from three layers:

1. **Base orchestrator instructions** — Role definition, autonomy level, escalation criteria, and operating principles. Includes explicit guidance: "You coordinate AI agent sessions. Use `ao` commands to spawn, monitor, nudge, and clean up sessions. Escalate to the human when: a session needs a decision that requires judgment beyond CI/review state, a circuit breaker trips, or a reaction loop exceeds retry limits."

2. **`ao` command reference** — Full CLI reference (all 10 MVP commands + post-MVP commands as they land), with `--json` examples for machine-readable output. Rendered from a static template updated alongside each new `ao` command.

3. **Current state snapshot** — `ao status --json` output captured at spawn time, plus static project metadata (IDs, repo names, paths), passed as `session_snapshot: Vec<SessionMetadata>` to `render_orchestrator()`. Mutable runtime constraints (concurrency limits, tracker state mappings) are excluded from the prompt; the orchestrator agent is instructed to query `ao status --json` for live state rather than relying on the initial snapshot.

Post-MVP enhancements (not in this ADR's scope): structured MCP tool definitions for `ao` commands (replacing shell invocation), reaction policy overrides from the orchestrator agent (agent posts a structured comment that the reaction engine reads), and `orchestratorRules` config field injection (already schema'd in FR10).

## Consequences

Positive:

- Reusing the Agent + Runtime plugin stack means zero new process management code. The orchestrator session is visible in `ao status`, attachable via `ao open`, and killable via `ao session kill` — all existing CLI commands work unchanged.
- The simplified lifecycle engine path is additive: `IS_ORCHESTRATOR=true` in metadata is a branch condition in the gather and evaluate phases. The core graph data structure is not modified.
- Auto-restart with a circuit breaker provides resilience without crash loops. The circuit breaker's notification path reuses the existing notifier infrastructure (ADR-0009 `notify` action).
- `PromptEngine::render_orchestrator()` is already defined (ADR-0008). This ADR connects the existing render method to a real invocation path.
- Shell invocation of `ao --json` commands is immediately testable: the orchestrator agent can be given test prompts in a controlled environment, and its `ao` command outputs can be inspected. No new protocol to debug.
- The `{prefix}-orchestrator` session ID convention is stable and unique per orchestrator instance. No collision with worker session IDs (which always include an issue ID segment).

Negative:

- The orchestrator worktree defaults to `sandbox: 'read-only'`. Orchestration artifacts (if any) are written to a dedicated directory `{data_path}/orchestrator-artifacts/` which is explicitly excluded from the sandbox restriction. This directory is outside the git worktree, preventing accidental commits. Teams that need the orchestrator to write to the repository directly can set `orchestratorSandbox: 'workspace-write'` with explicit acknowledgment of the risk.
- The orchestrator sandbox's read-only restriction applies to *source code files* (the working tree) but explicitly permits writes to the `.git/` metadata directory. This carve-out is required for `git fetch` (writes `FETCH_HEAD` and the object store) and `git checkout` (updates the index). The sandbox configuration must express this as "source-read-only" rather than "directory-read-only" — the implementation detail is that the Claude Code agent's bash tool is permitted to run `git` operations but not to write to non-`.git/` paths in the worktree. The orchestrator worktree is on the default branch at spawn time; it does not track branch updates automatically. The orchestrator agent can run `git fetch` and `git checkout` within these sandbox permissions, or re-read files via absolute paths.
- Shell invocation of `ao` commands creates a dependency on `ao` being in `PATH` within the tmux session. If the orchestrator agent's environment does not have `ao` in PATH (e.g., installed via `cargo install` but not in the agent's shell PATH), commands will fail. Mitigation: the orchestrator session's tmux environment is set with `AO_BIN_PATH` pointing to the discovered `ao` binary. The base prompt instructs the agent to use `$AO_BIN_PATH` if `ao` is not found in PATH.
- The orchestrator session's activity state detection (`Agent::detect_activity()`) is designed for issue-working agents. Between coordination tasks, the orchestrator agent will frequently be in `idle` or `ready` state — this is expected and not a signal of a problem. The lifecycle engine must not apply `stuck` detection to orchestrator sessions (this is handled by the simplified evaluate path, which has no `stuck` transition).
- Auto-restart increases orchestrator daemon complexity: a restart queue must be maintained alongside the in-memory reaction engine queue (ADR-0009). Both are ephemeral; both survive orchestrator crashes via re-derivation. Restart queue entries are re-derived by the startup reconciliation path (component 3) — detecting `IS_ORCHESTRATOR=true` sessions in `errored` state and re-evaluating the circuit breaker from `RESTART_TIMESTAMPS`. This is crash-safe without a separate persistent queue.
- Several new config fields are introduced (`maxOrchestratorRestarts`, `orchestratorRestartDelayMs`, `orchestratorRestartWindowMs`, `orchestratorActivityTimeoutMs`, `orchestratorSpawnRateLimitPerMinute`). Adding config fields has been low-friction in ADR-0003's typed struct approach, but each new field is a maintenance surface. All have sensible defaults and most teams will not need to change them.
- The initial scope of the orchestrator prompt (shell `ao` commands) requires the orchestrator agent to construct correct CLI syntax. A hallucinated `ao` flag or wrong session ID will result in a CLI error, not silent failure — the agent sees the error output and can correct. Post-MVP MCP tools would provide stronger type safety.

Reference `docs/plans/` for implementation pseudocode and startup sequence integration details.

---

## Council Review — Round 2 Findings Addressed

| ID | Severity | Summary | Resolution |
|----|----------|---------|------------|
| CF-1 | High | IPC handler does not guard against clone orchestrators | Added "Reserved identity guard" to IPC handler guards section: `Spawn`/`BatchSpawn` handlers explicitly reject requests with `*-orchestrator` session ID pattern or `IS_ORCHESTRATOR=true` metadata |
| CF-2 | High | Multi-project worktree repository unspecified | Spawn sequence step 2 now specifies: worktree tracks the primary project's repository (the project providing `orchestratorSessionPrefix`); multi-repo access via absolute paths or `ao status --json` |
| CF-3 | Medium | `IS_ORCHESTRATOR` casing inconsistency | Standardized to `IS_ORCHESTRATOR=true` (uppercase) throughout; fixed lowercase instance in ADR-0005 cross-reference with canonical casing note |
| CF-4 | Medium | Git operations contradict read-only sandbox default | Clarified that read-only sandbox applies to source code files only, with explicit `.git/` metadata write carve-out required for `git fetch`/`git checkout`; updated Consequences section |
| CC MEDIUM | Medium | No shutdown guard for auto-restart during `ao stop` | Added step 3 to the `errored` restart path: check `shutdown_tx` signal before enqueuing restart; suppress and log if daemon shutdown is pending |
| CC HIGH (late) | High | `{data_path}` is per-project FNV hash — ambiguous for multi-project orchestrator | Changed to `{orchestrator_root}` (global `~/.agent-orchestrator/`); `DataPaths` gains `orchestrator_root()` accessor; orchestrator metadata lives outside per-project hash directories |
| CC MEDIUM (late) | Medium | Precedence values conflict with ADR-0001 global band namespace | Added note: orchestrator precedence values are local-only, evaluated in isolated `IS_ORCHESTRATOR=true` branch |
| CC MEDIUM (late) | Medium | `LAST_RESTART_AT` redundant with `max(RESTART_TIMESTAMPS)`; split-write risk | Removed `LAST_RESTART_AT`; display uses `max(RESTART_TIMESTAMPS)` |
| CC MEDIUM (late) | Medium | `BatchSpawn` counting ambiguity in spawn rate limit | Specified: N tokens per batch; full batch rejected if limit exceeded |
| CC LOW (late) | Low | Two `working→errored` rules share destination; short-circuit behavior unclear | Added note: evaluation short-circuits after first match; process-death fires before activity-timeout |

## Council Review — Round 1 Findings Addressed

The following findings from the multi-model council review (Gemini + Codex) were addressed in this revision:

| ID | Severity | Summary | Resolution |
|----|----------|---------|------------|
| CC HIGH-1 | High | Incomplete state machine — `errored→spawning` transition missing; `errored` not classified as non-terminal for orchestrator | Added `errored→spawning` as a fourth transition in the evaluate phase table; explicitly classified `errored` as NON-TERMINAL for orchestrator sessions; updated spawn sequence step 1 |
| CC HIGH-2 | High | Cold-start recovery — `errored` orchestrator not handled on daemon startup | Added startup reconciliation section in Component 3 describing crash-safe restart re-derivation from `RESTART_TIMESTAMPS` |
| CF-1 | High | No spawn rate limit on IPC handler | Added `orchestratorSpawnRateLimitPerMinute` config field and IPC handler enforcement in Component 3 |
| CF-2 | High | Worker sessions could kill the orchestrator via IPC | Added worker-on-orchestrator `AO_SESSION` guard in IPC handler section of Component 3 |
| CF-3 | High | Orchestrator uses project root — git lock contention risk | Changed to dedicated read-only worktree at `{data_path}/orchestrator-workspace/`; updated Consequences accordingly |
| CF-4 | Medium | Prompt includes mutable runtime state baked in | Changed layer 3 to a state snapshot at spawn time with instruction to query `ao status --json` for live state; updated `render_orchestrator()` signature to take `session_snapshot: Vec<SessionMetadata>` |
| MEDIUM-1 | Medium | No hung orchestrator detection | Added `working→errored` transition on `orchestratorActivityTimeoutMs` idle timeout; added config field |
| MEDIUM-3 | Medium | Circuit breaker algorithm vague ("max 3 per hour") | Replaced with exact sliding-window algorithm using `RESTART_TIMESTAMPS` list; removed `RESTART_COUNT` field |
| LOW-1 | Low | `orchestratorSessionPrefix` default non-deterministic | Changed to lexicographically first project prefix; added stability guarantee note |
| LOW-2 | Low | No rationale for `PostLaunch` in spawn sequence | Added parenthetical rationale citing ADR-0004 constraint |
| LOW-3 | Low | Option 5 (MCP tools) not linked to FR17 | Added FR17 scoped-credential enforcement reference in Decision section |
| Gemini | Medium | Sandbox contradiction — "read-only" then "write access to main repository" | Replaced ambiguous text with explicit `orchestrator-artifacts/` directory outside worktree; added `orchestratorSandbox` config option |
