# ADR-0010: Orchestrator-as-Session

## Status
Accepted

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
3. **ADR-0005** — `SessionStore` stores the orchestrator session with ID `{prefix}-orchestrator`. `DataPaths` requires no changes — the orchestrator session's metadata lives in `{hash}/sessions/{prefix}-orchestrator/`. The orchestrator session has `is_orchestrator=true` in metadata.
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

**`ao` command invocation:** Option 4 — Shell execution for the MVP of this feature. Claude Code's bash tool runs `ao` commands with `--json`. Option 5 (MCP tools) is listed as a planned post-MVP enhancement — it would enable better error handling and structured round-trips, but adds scaffolding complexity. The shell path is immediately functional.

**Lifecycle on death:** Option 7 — Auto-restart with configurable backoff and a circuit breaker (max 3 restarts per hour, configurable). After the circuit breaker trips, notify the human and stop restarting.

**Session identity:** Option 10 — Fixed ID `{prefix}-orchestrator`. Simple, unambiguous, no synthetic hacks.

The design has four components:

### 1. Session Identity and Metadata

The orchestrator session has ID `{prefix}-orchestrator`. The `{prefix}` is derived from the config's `sessionPrefix` field (same derivation as worker sessions — first project in config or a global prefix). If multiple projects share one orchestrator, the prefix is the orchestrator-level prefix (`orchestratorSessionPrefix` config field, defaulting to the first project's prefix).

Session metadata includes a special field `IS_ORCHESTRATOR=true` that the lifecycle engine, session store, and CLI use to apply orchestrator-specific behavior:

- `ao session ls` labels the orchestrator session with `[orchestrator]` rather than an issue ID.
- `ao status` pins the orchestrator session row at the top of the output.
- The lifecycle engine applies simplified gather and recovery rules (see component 3).

The orchestrator session does not have `ISSUE_ID`, `BRANCH`, or `PR_NUMBER` metadata fields. It has `RESTART_COUNT` (incremented on each auto-restart) and `LAST_RESTART_AT` (epoch ms).

### 2. Spawn Sequence

The orchestrator session is spawned during `ao start` startup, after the poll loop and IPC listener are ready (step 8 of the ADR-0007 startup sequence becomes step 8a). This ordering ensures the orchestrator agent can immediately issue `ao` commands against a running orchestrator daemon.

Spawn sequence for the orchestrator session:

1. Check if a session with ID `{prefix}-orchestrator` already exists in `SessionStore` and is non-terminal. If so, skip spawn (idempotent).
2. Derive `workspace_path` — the orchestrator session uses the project root directly (no worktree). It reads and observes all projects but does not modify code.
3. Call `PromptEngine::render_orchestrator(config, projects)` — generates the system prompt.
4. Construct `LaunchPlan` via `Agent::launch_plan(LaunchParams { session_id, workspace_path, prompt, delivery: PromptDelivery::PostLaunch })`.
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
Only two transitions apply:

| From | To | Trigger | Precedence |
|------|----|---------|------------|
| `spawning` | `working` | Agent process detected as active | 1 |
| `working` | `errored` | Runtime not alive | 2 |
| any | `killed` | Manual `ao session kill {prefix}-orchestrator` | 0 |

No `stuck` detection (the orchestrator agent may be idle between coordination tasks). No `ci_failed`, `pr_open`, `changes_requested`, or other PR-lifecycle states.

**Transition phase (orchestrator session on `errored`):**
Entry action for `errored` on an orchestrator session: instead of notifying a human and stopping (as for worker sessions), schedule an auto-restart:

1. Read `RESTART_COUNT` from metadata.
2. Check circuit breaker: if `RESTART_COUNT` since last reset exceeds `maxOrchestratorRestarts` (default: 3) within `orchestratorRestartWindowMs` (default: 3600000, one hour), trip the breaker — notify human, do not restart.
3. Otherwise: queue a restart after `orchestratorRestartDelayMs` (default: 30000ms = 30s). The restart executes as a scheduled task within the poll loop at the next eligible tick, re-running the spawn sequence (component 2, step 1's idempotency check is skipped on restart).

### 4. Orchestrator Prompt

`PromptEngine::render_orchestrator()` composes the orchestrator system prompt from three layers:

1. **Base orchestrator instructions** — Role definition, autonomy level, escalation criteria, and operating principles. Includes explicit guidance: "You coordinate AI agent sessions. Use `ao` commands to spawn, monitor, nudge, and clean up sessions. Escalate to the human when: a session needs a decision that requires judgment beyond CI/review state, a circuit breaker trips, or a reaction loop exceeds retry limits."

2. **`ao` command reference** — Full CLI reference (all 10 MVP commands + post-MVP commands as they land), with `--json` examples for machine-readable output. Rendered from a static template updated alongside each new `ao` command.

3. **Current project list** — Project IDs, repo names, active tracker states, and concurrency limits from `Config`. Allows the orchestrator agent to make project-aware dispatch decisions.

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

- The orchestrator session uses the project root as its workspace (no worktree). It has write access to the main repository. This is intentional (the orchestrator agent may need to read config, logs, or create orchestration artifacts) but means a misconfigured or runaway orchestrator agent could modify main-branch files. Mitigation: the orchestrator agent's `agentConfig.sandbox` should default to `"read-only"` (configurable to `"workspace-write"` for teams that want the orchestrator to create planning files). This is enforced by the Agent plugin's launch configuration, not by a runtime policy.
- Shell invocation of `ao` commands creates a dependency on `ao` being in `PATH` within the tmux session. If the orchestrator agent's environment does not have `ao` in PATH (e.g., installed via `cargo install` but not in the agent's shell PATH), commands will fail. Mitigation: the orchestrator session's tmux environment is set with `AO_BIN_PATH` pointing to the discovered `ao` binary. The base prompt instructs the agent to use `$AO_BIN_PATH` if `ao` is not found in PATH.
- The orchestrator session's activity state detection (`Agent::detect_activity()`) is designed for issue-working agents. Between coordination tasks, the orchestrator agent will frequently be in `idle` or `ready` state — this is expected and not a signal of a problem. The lifecycle engine must not apply `stuck` detection to orchestrator sessions (this is handled by the simplified evaluate path, which has no `stuck` transition).
- Auto-restart increases orchestrator daemon complexity: a restart queue must be maintained alongside the in-memory reaction engine queue (ADR-0009). Both are ephemeral; both survive orchestrator crashes via re-derivation. Restart queue entries are simply re-derived by detecting `IS_ORCHESTRATOR=true` sessions in `errored` state on startup — the same re-poll recovery already used for worker sessions.
- `maxOrchestratorRestarts` and `orchestratorRestartDelayMs` are new config fields. Adding config fields has been low-friction in ADR-0003's typed struct approach, but each new field is a maintenance surface. Both have sensible defaults and most teams will not need to change them.
- The initial scope of the orchestrator prompt (shell `ao` commands) requires the orchestrator agent to construct correct CLI syntax. A hallucinated `ao` flag or wrong session ID will result in a CLI error, not silent failure — the agent sees the error output and can correct. Post-MVP MCP tools would provide stronger type safety.

Reference `docs/plans/` for implementation pseudocode and startup sequence integration details.
