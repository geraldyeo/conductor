---
version: "1.3"
date: 2026-03-06
status: Accepted
---

# Product Requirements Document (PRD): Agent Orchestrator

## 1. Executive Summary

**Vision:** To provide a scalable, parallel, and autonomous orchestration layer for AI coding agents (Claude Code, Aider, Codex, OpenCode, Gemini CLI, OpenClaw), enabling them to work on complex engineering tasks with the same isolation and lifecycle management as human developers.

The orchestrator combines two approaches: it actively manages PR/CI/review lifecycle (like ComposioHQ's agent-orchestrator) while also supporting a scheduler/reader model where agents self-manage tracker state (inspired by OpenAI's Symphony). Teams choose the level of orchestrator involvement that suits their workflow.

**CLI binary:** `ao`
**Config file:** `agent-orchestrator.yaml`
**Data directory:** `~/.agent-orchestrator/`

## 2. Problem Statement

AI coding agents are highly effective for individual tasks but face significant hurdles in a professional team environment:

- **Workspace Conflicts:** Running multiple agents in a single git clone causes branch and state collisions.
- **Monitoring Overhead:** Developers must manually watch CI logs and PR comments to "nudge" agents.
- **Context Loss:** Agents often lose track of the broader project conventions and issue-tracking metadata.
- **Lack of Parallelism:** Teams cannot easily spawn 5 agents to fix 5 different bugs simultaneously.
- **Cost Blindness:** No visibility into token usage across agents, making spend unmanageable at scale.

## 3. Target Audience

- **Software Engineering Teams:** Looking to automate maintenance, CI fixes, and minor feature requests.
- **DevOps/Platform Engineers:** Building automated "self-healing" CI pipelines.
- **AI Tooling Developers:** Seeking a robust runtime to host their agents.

## 4. Functional Requirements (FRs)

### FR1: Multi-Agent Support (Agnosticism)

- Must support multiple agent backends through a standardized `Agent` interface.
- Implemented agents: `claude-code` (default), `codex`, `aider`, `opencode`, `gemini`, `openclaw`.
- Each agent plugin exposes: `getLaunchCommand()`, `getEnvironment()`, `getActivityState()`, `isProcessRunning()`, `getSessionInfo()`, and optionally `getRestoreCommand()`, `postLaunchSetup()`, `setupWorkspaceHooks()`.
- Must support multiple prompt delivery modes via `promptDelivery` property:
  - `"inline"`: prompt passed as CLI argument (default).
  - `"post-launch"`: prompt sent via `runtime.sendMessage()` after agent starts in interactive mode. Required for agents that exit after one-shot when given an inline prompt (e.g., Claude Code with `-p`).
  - `"protocol"`: prompt sent via a structured protocol (e.g., Agent Client Protocol, JSON-RPC over stdio). For agents like OpenClaw (via `acpx`) or Codex (via `app-server`) that expose programmatic session interfaces rather than terminal I/O.
- Must support **multi-turn sessions**: an agent run can execute up to `maxTurns` turns (default: 20) on the same thread, re-checking issue state between turns. If the issue is still active after a turn completes, the agent continues with continuation guidance rather than re-sending the full prompt. This is more efficient than single-shot + reaction loops.
- Must support **continuation retries**: after a successful agent run, the orchestrator schedules a short-delay re-check. If the issue is still active, a new session is spawned automatically.
- Must allow per-project agent configuration:
  - `agentConfig.permissions`: `"skip"` or `"default"` permission mode.
  - `agentConfig.model`: model override (e.g., specific Claude or GPT model).
  - `agentConfig.maxTurns`: maximum turns per session (default: 20).
  - `agentConfig.sandbox`: filesystem access policy (`"workspace-write"`, `"read-only"`, `"full"`). Default: `"workspace-write"`.
  - `agentRules`: inline rules injected into agent prompts.
  - `agentRulesFile`: path to external rules file (relative to project root).

### FR2: Isolated Parallel Workspaces

- Must use `git worktree` (primary) or `git clone` to create isolated environments for every agent session.
- Workspace interface: `create()`, `destroy()`, `list()`, optionally `postCreate()`, `exists()`, `restore()`.
- Must support **symlink** configuration for sharing files across worktrees (e.g., `.env`, `.claude`).
- Must support **4 lifecycle hooks** as shell commands:
  - `afterCreate`: runs once on first workspace creation (e.g., `git clone`, dependency installation).
  - `beforeRun`: runs before every agent attempt (e.g., branch sync, cleanup stale artifacts).
  - `afterRun`: runs after every agent attempt (e.g., metrics collection, cache cleanup).
  - `beforeRemove`: runs before workspace deletion (e.g., save logs, archive artifacts).
- Must support **agent-specific workspace hooks** (e.g., Claude Code's PostToolUse metadata updater script that auto-detects `gh pr create`, `git checkout -b`, and `gh pr merge` commands to update session metadata).
- **Symlink escape prevention**: workspace paths must be validated by resolving symlinks and verifying they remain within the workspace root.
- Directory isolation via hash-based paths: `~/.agent-orchestrator/{sha256-12chars}-{projectId}/`. Multiple orchestrator instances with different configs can coexist without collision. `.origin` files detect hash collisions.

### FR3: "Eight Slot" Plugin Architecture

The system is modular across eight dimensions. Each plugin implements a `PluginModule` contract:

Each plugin exports a manifest (name, slot, description, version) and a factory function.

| # | Slot | Interface | Implementations | Planned/Listed |
|---|------|-----------|-----------------|----------------|
| 1 | **Runtime** | `create()`, `destroy()`, `sendMessage()`, `getOutput()`, `isAlive()`, optional `getMetrics()`, `getAttachInfo()` | `tmux` (default), `process` | Docker, K8s, SSH, E2B |
| 2 | **Agent** | See FR1 | `claude-code` (default), `codex`, `aider`, `opencode`, `gemini`, `openclaw` | Goose |
| 3 | **Workspace** | `create()`, `destroy()`, `list()`, optional `postCreate()`, `exists()`, `restore()` | `worktree` (default), `clone` | — |
| 4 | **Tracker** | `getIssue()`, `isCompleted()`, `issueUrl()`, `branchName()`, `generatePrompt()`, optional `listIssues()`, `updateIssue()`, `createIssue()`, `issueLabel()` | `github` (default), `linear` | Jira |
| 5 | **SCM** | `detectPR()`, `getPRState()`, `mergePR()`, `closePR()`, `getCIChecks()`, `getCISummary()`, `getReviews()`, `getReviewDecision()`, `getPendingComments()`, `getAutomatedComments()`, `getMergeability()`, optional `getPRSummary()` | `github` | GitLab |
| 6 | **Notifier** | `notify()`, optional `notifyWithActions()`, `post()` | `desktop`, `slack`, `composio`, `webhook` | — |
| 7 | **Terminal** | `openSession()`, `openAll()`, optional `isSessionOpen()` | `iterm2`, `web` | — |
| 8 | **Lifecycle** | `start()`, `stop()`, `getStates()`, `check()` | Core (not pluggable) | — |

### FR4: Autonomous Reactions

A rule-based engine maps event types to actions. Three action types: `send-to-agent`, `notify`, `auto-merge`.

| Reaction | Trigger | Default Action | Retry/Escalation |
|----------|---------|----------------|------------------|
| `ci-failed` | CI checks fail on PR | Send fix instructions to agent | 2 retries, escalate to human after 2 failures |
| `changes-requested` | Reviewer requests changes | Send review comments to agent | Escalate after 30 min |
| `bugbot-comments` | Automated bot posts review feedback | Send bot feedback to agent | Escalate after 30 min |
| `merge-conflicts` | PR has merge conflicts | Send rebase instructions to agent | Escalate after 15 min |
| `approved-and-green` | PR approved + CI green | Notify human (optionally auto-merge) | — |
| `agent-stuck` | Agent inactive beyond threshold | Notify human urgently | Threshold: 10 min |
| `agent-needs-input` | Agent asking question/permission | Notify human urgently | — |
| `agent-exited` | Agent process exited | Notify human urgently | — |
| `all-complete` | All sessions in project are done | Notify with summary | — |
| `tracker-terminal` | Issue moved to terminal state in tracker | Kill agent, clean up workspace | — |
| `rework-requested` | Issue enters rework state | Close PR, fresh branch from main, restart | — |

- Reactions are configurable at the global level and overridable per-project.
- The reaction tracker maintains per-session attempt counts for retry logic.
- **Idempotency**: every automated mutation (merge, close, restart, cleanup) must be idempotent. The orchestrator maintains an **action journal** — a per-session append-only log of executed actions with dedupe keys (action type + target + timestamp window). Before executing a destructive action, the orchestrator checks the journal and skips if the same action was already performed within the dedupe window. This prevents repeated PR churn from polling retries or reaction re-triggers.
- **Wait-for-ready protocol**: when a non-terminal event occurs (CI failure, review comments, merge conflicts) while an agent is mid-turn (activity state = `active`), the orchestrator queues the nudge and delivers it only when the agent reaches `ready` or `idle` state. This prevents disrupting the agent's current reasoning loop. Terminal events (issue closed, budget exceeded) bypass this and trigger immediate action.
- **Exponential backoff**: failed runs retry with `min(base * 2^attempt, maxRetryBackoffMs)`. Default max: 5 minutes. Prevents agent thrashing on persistent failures.
- **Auto-cancellation**: when a tracked issue moves to a terminal state (closed, cancelled, done), the agent is automatically killed and the workspace is cleaned up.
- **Rework flow**: when an issue enters a rework state, the orchestrator closes the existing PR, creates a fresh branch from the default branch, and restarts the agent from scratch — avoiding incremental patches on rejected approaches.
- **Budget enforcement**: the orchestrator monitors per-session token usage and wall-clock time against configured limits (`maxSessionTokens`, `maxSessionWallClockMs`). If a session exceeds either limit, it transitions to `killed` status with `terminationReason=budget_exceeded` in the session metadata, and the human is notified. Per-issue retry attempts are capped by `maxRetriesPerIssue` (default: 5 per day) to prevent runaway cost loops.

### FR5: Scheduling & Concurrency

- **Global concurrency limit**: `maxConcurrentAgents` (default: 10). Caps total running agent sessions.
- **Per-state concurrency limits**: `maxConcurrentAgentsByState` allows limiting how many agents work on issues in a given tracker state (e.g., max 3 on "In Progress", max 2 on "Review"). Prevents resource starvation where one stage monopolizes all agents.
- **Priority-based dispatch**: when multiple issues are eligible, agents are assigned by issue priority (highest first), then by age (oldest first). Higher-priority work gets agents before lower-priority work.
- **Blocker/dependency awareness**: issues with non-terminal blockers or unresolved dependencies in the tracker are not dispatched until their blockers resolve.
- **Future: inter-agent conflict detection**: worktrees prevent filesystem conflicts but not logical merge conflicts. A future enhancement could scan active sessions for overlapping file modifications and sequence or warn when two agents touch the same files.

### FR6: CLI (`ao`)

| Command | Description | Key Options |
|---------|-------------|-------------|
| `ao init` | Interactive setup wizard; creates `agent-orchestrator.yaml` | `--auto` (no prompts, smart defaults), `--smart` (analyze project for custom rules), `-o <path>` |
| `ao start [project\|url]` | Start orchestrator agent + web dashboard. Accepts a repo URL for one-command onboarding. | `--no-dashboard`, `--no-orchestrator`, `--rebuild` |
| `ao stop [project]` | Stop orchestrator agent and dashboard | — |
| `ao status` | Show all sessions with branch, PR, CI, review, activity, age, tokens | `-p <project>`, `--json` |
| `ao spawn <project> [issue]` | Spawn a single agent session | `--open` (open terminal tab), `--agent <name>` |
| `ao batch-spawn <project> <issues...>` | Spawn sessions for multiple issues with duplicate detection | `--open` |
| `ao send <session> [message...]` | Send message to session with busy detection, idle-wait, and delivery verification | `-f <file>`, `--no-wait`, `--timeout <seconds>` (default 600) |
| `ao session ls` | List all sessions | `-p <project>` |
| `ao session kill <session>` | Kill session and remove worktree | — |
| `ao session cleanup` | Kill sessions where PR is merged or issue is closed | `-p <project>`, `--dry-run` |
| `ao session restore <session>` | Restore a terminated/crashed session in-place | — |
| `ao review-check [project]` | Check PRs for unresolved review threads via GraphQL | `--dry-run` |
| `ao dashboard` | Start web dashboard standalone | `-p <port>`, `--no-open`, `--rebuild` |
| `ao open [target]` | Open session(s) in iTerm2 tabs | `-w` (new window) |

**Send command intelligence:** `ao send` waits for the agent to be idle (up to configurable timeout), clears partial input (Ctrl-U), handles large messages via tmux buffer, and verifies delivery with 3 retry attempts checking for "active" or "queued" states.

**Batch spawn behavior:** Duplicate detection against existing sessions AND within same batch. Skips dead/killed sessions (allows re-spawning crashed issues). Pre-flight checks (tmux, gh auth) run once before loop. 500ms delay between spawns.

**Session restore:** Revives crashed/terminated sessions in-place by reusing the existing workspace and creating a new runtime with the agent's resume command (if available). Guards against restoring merged sessions.

### FR7: Web Dashboard

- **Real-time updates:** Server-Sent Events with 5-second polling intervals.
- **WebSocket terminal:** Direct terminal access via WebSocket.
- **Attention zones:** Sessions classified into 6 priority levels: merge (highest ROI), respond, review, pending, working, done.
- **Token tracking:** Per-session and aggregate input/output token counters with throughput sparklines. Essential for cost visibility when running many agents.
- **Dynamic favicon:** Reflects aggregate session state.
- **Rate-limiting awareness:** PR enrichment has 3-4 second timeout; degrades gracefully with stale data rather than blocking.

**API routes:**
- `GET /api/sessions` — list all sessions
- `GET /api/sessions/[id]` — session detail (includes token usage)
- `POST /api/sessions/[id]/send` — send message to session
- `POST /api/sessions/[id]/kill` — kill session
- `POST /api/sessions/[id]/restore` — restore session
- `POST /api/spawn` — spawn new session
- `POST /api/prs/[id]/merge` — merge PR
- `GET /api/events` — SSE stream
- `GET /api/state` — full system state snapshot
- `POST /api/refresh` — trigger immediate poll cycle

### FR8: Terminal Status Dashboard

A rich ANSI terminal UI for monitoring without a browser:

- Running/retrying/completed session counts.
- Per-session details: identifier, state, age, tokens, last activity.
- Retry queue with attempt counts, due times, and error summaries.
- Token usage: input/output/total with tokens-per-second sparkline graphs.
- Throughput history (configurable window, sparkline chart).
- Auto-refreshing at configurable intervals.

### FR9: Mobile Companion App

- Session cards, stat bar, spawn screen, terminal screen, settings.
- Push notification support with background tasks.
- Backend context provider for API communication.

### FR10: Configuration System

**Config file:** `agent-orchestrator.yaml` (or `.yml`), validated at load time.

**Hot-reload:** The orchestrator watches the config file for changes and applies updates without restart. Invalid changes are rejected, keeping the last-known-good configuration. This covers polling intervals, concurrency limits, reaction thresholds, and prompt rules.

**Config search order:**
1. `AO_CONFIG_PATH` environment variable
2. Walk up directory tree from CWD (like git)
3. Explicit `startDir` parameter
4. Home directory: `~/.agent-orchestrator.yaml`, `~/.agent-orchestrator.yml`, `~/.config/agent-orchestrator/config.yaml`

**Top-level options:**

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `port` | number | 3000 | Web dashboard port |
| `terminalPort` | number | auto | Terminal WebSocket server port |
| `directTerminalPort` | number | auto | Direct terminal WebSocket port |
| `readyThresholdMs` | number | 300000 (5 min) | Ms before "ready" session becomes "idle" |
| `maxConcurrentAgents` | number | 10 | Global cap on running agent sessions |
| `maxConcurrentAgentsByState` | Record | `{}` | Per-tracker-state concurrency limits |
| `maxRetryBackoffMs` | number | 300000 (5 min) | Cap on exponential backoff delay |
| `maxSessionTokens` | number | — | Max tokens (input+output) per session before auto-kill |
| `maxSessionWallClockMs` | number | — | Max wall-clock time per session before auto-kill |
| `maxRetriesPerIssue` | number | 5 | Max total retry attempts per issue per day |
| `defaults.runtime` | string | `"tmux"` | Default runtime plugin |
| `defaults.agent` | string | `"claude-code"` | Default agent plugin |
| `defaults.workspace` | string | `"worktree"` | Default workspace plugin |
| `defaults.notifiers` | string[] | `["composio", "desktop"]` | Default notifier channels |
| `projects` | Record | required | Project configurations |
| `notifiers` | Record | `{}` | Notifier channel configs (e.g., Slack webhook URL) |
| `notificationRouting` | Record | see FR12 | Route events by priority to channels |
| `reactions` | Record | see FR4 | Global reaction configs |

**Per-project options:**

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `name` | string | project ID | Display name |
| `repo` | string | required | GitHub repo `"owner/repo"` |
| `path` | string | required | Local path (supports `~`) |
| `defaultBranch` | string | `"main"` | Default git branch |
| `sessionPrefix` | string | auto-derived | Session name prefix |
| `runtime` | string | from defaults | Override runtime |
| `agent` | string | from defaults | Override agent |
| `workspace` | string | from defaults | Override workspace |
| `tracker.plugin` | string | `"github"` | Tracker plugin |
| `tracker.teamId` | string | — | Linear team ID |
| `tracker.activeStates` | string[] | — | Tracker states that trigger agent dispatch |
| `tracker.terminalStates` | string[] | — | Tracker states that trigger cleanup |
| `scm.plugin` | string | `"github"` | SCM plugin |
| `symlinks` | string[] | — | Files/dirs to symlink into worktrees |
| `hooks.afterCreate` | string | — | Shell command after workspace creation |
| `hooks.beforeRun` | string | — | Shell command before each agent run |
| `hooks.afterRun` | string | — | Shell command after each agent run |
| `hooks.beforeRemove` | string | — | Shell command before workspace deletion |
| `agentConfig` | object | — | See FR1 |
| `reactions` | Record | — | Per-project reaction overrides |
| `agentRules` | string | — | Inline rules for agent prompts |
| `agentRulesFile` | string | — | Path to rules file |
| `orchestratorRules` | string | — | Rules for the orchestrator agent |

**Environment variables:**

| Variable | Purpose |
|----------|---------|
| `AO_CONFIG_PATH` | Override config file location |
| `LINEAR_API_KEY` | Linear tracker integration |
| `SLACK_WEBHOOK_URL` | Slack notifier |
| `COMPOSIO_API_KEY` | Composio notifier |
| `AO_SESSION` | Per-session metadata identifier (set automatically) |
| `AO_DATA_DIR` | Per-session data directory (set automatically) |

### FR11: Prompt System

Agent prompts are composed from multiple layers:

1. **Base prompt** (`BASE_AGENT_PROMPT`): Constant instructions covering lifecycle behavior, git workflow conventions, and PR practices. Applied to all agents.
2. **Config-derived context**: Injected automatically — project info, issue details, tracker metadata, reaction hints.
3. **Skills directory**: Structured markdown skill definitions in `.ao/skills/` (e.g., `commit.md`, `debug.md`, `land.md`, `review.md`). Richer and more composable than inline rules — each skill is a focused instruction set the agent can reference.
4. **User rules**: Custom instructions from `agentRules` (inline) or `agentRulesFile` (external file), merged into the prompt.
5. **Template rendering**: Prompt bodies support template variables (`{{ issue.title }}`, `{{ issue.description }}`, `{{ project.name }}`) for dynamic context injection.

**Dynamic tools:** The orchestrator can advertise client-side tools to agents at session start (e.g., `tracker_graphql` for raw tracker API access). Agents call tools via their protocol, and the orchestrator executes them. This enables agents to self-manage tracker state without the orchestrator mediating every mutation.

**Workpad pattern:** Agents are instructed to maintain a persistent structured progress comment on each issue as a running status tracker (plan, acceptance criteria, validation status, notes). This provides visibility without polling the agent's terminal output.

### FR12: Notification Routing

Events are classified by priority. Each priority level routes to configurable notifier channels.

**Default routing:**

| Priority | Channels |
|----------|----------|
| `urgent` | `desktop`, `composio` |
| `action` | `desktop`, `composio` |
| `warning` | `composio` |
| `info` | `composio` |

Routing is configurable at the top level via `notificationRouting`.

### FR13: Orchestrator-as-Session

`ao start` creates a special orchestrator session (suffixed `-orchestrator`) that is itself an AI agent. This orchestrator agent receives a system prompt generated by `generateOrchestratorPrompt()` containing:

- Full `ao` command reference
- Session management workflows
- Dashboard information
- Common workflows and tips

This enables fully autonomous operation where an AI agent coordinates other AI agents.

### FR14: One-Command Onboarding

`ao start <url>` provides a full onboarding flow:

1. **URL parsing:** Supports GitHub, GitLab, Bitbucket URLs (HTTPS + SSH formats).
2. **Auth fallback chain:** `gh repo clone` -> SSH -> HTTPS.
3. **Project detection:** Auto-detects language (TypeScript, JavaScript, Python, Go, etc.), framework (React, Next.js, Vue, Express), test framework (Vitest, Jest, Mocha), and package manager (pnpm, yarn, npm).
4. **Rule generation:** Generates language-specific agent rules from templates (base, TypeScript, Python, Go, React, Next.js, monorepo-workspaces).
5. **Config generation:** Creates `agent-orchestrator.yaml` with detected settings.
6. **Auto-port selection:** Finds a free port if the default (3000) is busy.
7. **Start:** Launches dashboard + orchestrator.

### FR15: Session Metadata System

- Flat key=value files (bash-compatible format).
- **Atomic writes** via temp file + rename.
- **Archive on delete:** Timestamped copies preserved.
- **Race-free creation:** Session ID reservation with `O_EXCL` flag.
- **Path traversal prevention:** Session ID validation.
- **Token tracking:** Per-session input/output token counters, updated during agent runs. Aggregated for dashboard and cost reporting.
- **Termination reason:** When a session reaches a terminal status, a `terminationReason` field records the cause (e.g., `budget_exceeded`, `manual_kill`, `stall_timeout`, `tracker_terminal`, `agent_exit`). This distinguishes different kill causes without adding statuses.
- **Action journal:** Per-session append-only log of orchestrator-executed actions (merge, close, restart, label, cleanup). Each entry contains: action type, target (PR/issue ID), timestamp, dedupe key, result (`success` | `failed` | `skipped`), error code (if failed), attempt number, and actor (`orchestrator` | `reaction_engine` | `human`). Used by FR4 for idempotency checks and by the reaction engine for retry/escalation decisions.

### FR16: Tracker Integration

- Must support multiple tracker backends through the Tracker plugin slot (GitHub Issues, Linear, Jira).
- **Active/terminal state mapping:** Configurable `activeStates` (issue states that trigger agent dispatch) and `terminalStates` (issue states that trigger cleanup). Allows the orchestrator to auto-dispatch from the tracker without manual `ao spawn`.
- **Workspace cleanup on terminal state:** When an issue reaches a terminal state, the orchestrator kills the agent (if running) and cleans up the workspace.
- **Blocker/dependency awareness:** Issues with non-terminal blockers or unresolved dependencies are excluded from dispatch until blockers resolve.

### FR17: Mutation Authority Model

Defines who can mutate shared state (tracker issues, PRs, sessions) to prevent conflicts between the orchestrator and agents.

**Ownership principle:** Split by domain — agents own work-level mutations, the orchestrator owns lifecycle mutations. Enforcement is **mechanical** (tool-level), not prompt-level, for all critical actions.

**Enforcement mechanism:** The dynamic tools system (FR11) controls what each agent can do. Tools for lifecycle actions are **not advertised** to agents, so agents literally cannot perform them. Prompt-level guidance is used only for non-critical grey areas.

**Mutation ownership table:**

| Action | Owner | Enforcement | Rationale |
|--------|-------|-------------|-----------|
| Commit & push code | Agent | Tool provided | Core agent work |
| Create PR | Agent | Tool provided | Agent opens PR for its work |
| Update PR (push commits) | Agent | Tool provided | Agent iterates on its work |
| Comment on issue (work updates) | Agent | Tool provided | Agent reports progress |
| Request review | Agent | Tool provided | Agent signals work is ready |
| Merge PR | Orchestrator | Tool **withheld** | Lifecycle action; requires policy checks (approvals, CI) |
| Close PR | Orchestrator | Tool **withheld** | Lifecycle action; tied to reaction engine |
| Close issue | Orchestrator | Tool **withheld** | Lifecycle action; tied to terminal state detection |
| Apply/remove labels | Orchestrator | Tool **withheld** | Used for state signaling and reaction triggers |
| Spawn new sessions | Orchestrator | Tool **withheld** | Scheduling decision (FR5) |
| Kill / restart sessions | Orchestrator | Tool **withheld** | Lifecycle management |
| Rebase / fresh branch | Orchestrator | Tool **withheld** | Rework flow (FR4) |

**Conflict resolution rule:** If both orchestrator and agent could act on the same event, the orchestrator defers to a running agent. If no agent session is active, the orchestrator acts directly.

**Runtime command policy:** Tool withholding alone is insufficient — agents with shell access can bypass tool-level controls by executing commands directly (e.g., `gh pr merge`). To close this gap, the runtime plugin must enforce a **command policy** for worker sessions:

- **Blocked command patterns**: a configurable denylist of command families that worker agents must not execute (e.g., `gh pr merge`, `gh pr close`, `gh issue close`, `gh label`). The runtime intercepts or wraps shell execution to enforce this.
- **Scoped credentials**: worker sessions receive API tokens scoped to work-level permissions only (e.g., repo read/write but not PR merge). Orchestrator-only mutations use a separate, higher-privilege token. This provides defense-in-depth even if command blocking is bypassed.
- **Prompt reinforcement**: the base prompt (FR11) instructs agents not to perform lifecycle actions directly, as a soft additional layer.

The combination of tool withholding + command policy + scoped credentials provides three layers of enforcement: protocol-level, shell-level, and credential-level.

**Grey areas** (e.g., agent adding a "ready for review" comment) are handled via prompt guidance. The worst case for a grey-area violation is a redundant action, not a destructive one — all destructive actions are mechanically prevented.

## 5. Session Lifecycle

### 5.1 Session Statuses (16 states)

```
spawning -> working -> pr_open -> review_pending -> approved -> mergeable -> merged
                |         |           |               |
                |         +-> ci_failed -----(auto-fix)----> working
                |         +-> changes_requested -(auto)----> working
                |
                +-> needs_input    (human intervention required)
                +-> stuck          (agent inactive beyond threshold)
                +-> errored
                +-> killed         (manually or runtime died)
                +-> done
                +-> terminated
                +-> cleanup
```

**Terminal (dead) statuses:** `killed`, `terminated`, `done`, `cleanup`, `errored`, `merged`.
**Non-restorable:** `merged` (cannot `ao session restore` a merged session).

### 5.2 Activity States (6 states)

Detected by the agent plugin via JSONL log parsing (preferred) or terminal output parsing (fallback):

| State | Meaning |
|-------|---------|
| `active` | Agent is processing |
| `ready` | Agent finished its turn, waiting for input |
| `idle` | Inactive past `readyThresholdMs` (default 5 min) |
| `waiting_input` | Agent asking a question or requesting permission |
| `blocked` | Agent hit an error |
| `exited` | Agent process no longer running |

### 5.3 State Transition Table

The following table is the **single source of truth** for valid session status transitions. Any transition not listed is invalid. Transitions are evaluated in precedence order (top to bottom) during each poll cycle.

| From | To | Trigger | Precedence |
|------|----|---------|------------|
| `spawning` | `working` | Agent process detected as active | 1 |
| `spawning` | `errored` | Agent failed to start within timeout | 2 |
| `working` | `pr_open` | PR detected by branch name | 3 |
| `working` | `needs_input` | Activity state = `waiting_input` | 4 |
| `working` | `stuck` | Activity state = `idle` beyond threshold | 5 |
| `working` | `errored` | Activity state = `blocked` | 6 |
| `working` | `killed` | Runtime not alive | 7 |
| `working` | `done` | Agent exited cleanly, issue in terminal state | 8 |
| `working` | `terminated` | Agent exited, issue still active | 9 |
| `pr_open` | `ci_failed` | CI checks failed | 10 |
| `pr_open` | `review_pending` | CI green, review requested | 11 |
| `pr_open` | `working` | Agent still active (pushing new commits) | 12 |
| `pr_open` | `killed` | Runtime not alive | 13 |
| `ci_failed` | `working` | Agent detected as active (auto-fixing) | 14 |
| `ci_failed` | `killed` | Runtime not alive | 15 |
| `review_pending` | `changes_requested` | Reviewer requests changes | 16 |
| `review_pending` | `approved` | Review approved | 17 |
| `review_pending` | `ci_failed` | New CI failure on updated PR | 18 |
| `changes_requested` | `working` | Agent detected as active (addressing feedback) | 19 |
| `changes_requested` | `killed` | Runtime not alive | 20 |
| `approved` | `mergeable` | CI green + no merge conflicts | 21 |
| `approved` | `ci_failed` | CI failure after approval | 22 |
| `mergeable` | `merged` | PR merged (by orchestrator or human) | 23 |
| `needs_input` | `working` | Human sends message, agent resumes | 24 |
| `needs_input` | `killed` | Runtime not alive | 25 |
| `stuck` | `working` | Agent resumes activity | 26 |
| `stuck` | `killed` | Stall timeout exceeded, agent killed | 27 |
| any | `killed` | Manual `ao session kill` | 28 |
| any | `cleanup` | Issue reached terminal tracker state | 29 |
| any non-terminal | `killed` | Budget exceeded (`maxSessionTokens` / `maxSessionWallClockMs`) | 30 |

**Precedence rule:** When multiple transitions are valid in the same poll cycle, the lowest precedence number wins.

**Activity state vs session status:** Activity states (`active`, `ready`, `idle`, `waiting_input`, `blocked`, `exited`) are inputs to the transition table, not statuses themselves. A session has exactly one status at any time.

### 5.4 Status Determination Logic

The **transition table (Section 5.3) is the sole authority** for status changes. The determination algorithm gathers inputs and then evaluates the table:

1. **Gather inputs** (order does not imply precedence):
   - Runtime liveness (alive / not alive)
   - Budget state (within limits / exceeded)
   - Agent activity state via JSONL (preferred) or terminal output parsing (fallback)
   - PR association (auto-detect by branch name if not yet associated)
   - If PR exists: PR state (merged/closed), CI status, review decision, merge readiness
   - Tracker issue state (active / terminal)
2. **Evaluate transition table** in precedence order (lowest number wins) using gathered inputs.
3. **Apply first matching transition.** If no transition matches and agent is active, status remains unchanged.

This two-phase approach (gather then evaluate) ensures that the precedence table — not the input-gathering order — determines which transition fires.

### 5.5 Retry Behavior

- **Exponential backoff:** Failed sessions retry with `delay = min(base * 2^attempt, maxRetryBackoffMs)`. Default max: 5 minutes.
- **Continuation:** Successful sessions that leave the issue in an active tracker state trigger a short-delay re-check and potential new session.
- **Stall detection:** If no agent output is detected within the stall timeout, the agent is killed and retried.

## 6. Non-Functional Requirements (NFRs)

- **Extensibility:** New plugins are added by implementing the interface for the target slot.
- **Low Latency:** The lifecycle polling loop runs at a configurable interval (default 30 seconds). Re-entrancy guarded (skips if previous poll is still running). Concurrent session checks.
- **Persistence:** Session state stored in `~/.agent-orchestrator/` as flat metadata files. Survives orchestrator restarts. Atomic writes prevent corruption. Complemented by tracker-driven recovery (re-poll tracker for active issues on restart).
- **Isolation:** Hash-based directory namespacing allows multiple orchestrator instances to coexist. Symlink escape prevention on workspace paths.
- **Resilience:** Dashboard degrades gracefully under API rate limits. Send command retries delivery verification. Reactions track attempt counts for escalation. Exponential backoff prevents thrashing on persistent failures.
- **Cost Visibility:** Per-session and aggregate token tracking provides spend awareness across all agents.

## 7. Security Considerations

The following areas are acknowledged as important for production and enterprise use but are deferred beyond the initial implementation:

- **Authentication & authorization**: Dashboard APIs, WebSocket terminal, and session mutation endpoints currently have no authn/authz requirements. A future iteration should add token-based authentication and role-based access control (RBAC) for multi-user environments.
- **Agent sandboxing**: The `agentConfig.sandbox` option (`"workspace-write"`, `"read-only"`, `"full"`) controls filesystem access policy, but enforcement depends on the agent and runtime plugin. Stronger isolation (e.g., container-based runtimes) is listed as planned.
- **Tool allowlisting**: Dynamic tools (FR11) and shell hooks (FR2) execute arbitrary commands. A future allowlist/denylist mechanism should restrict which tools and hook commands are permitted per project.
- **Audit logging**: The action journal (FR15) provides a per-session log of orchestrator mutations. A centralized audit log aggregating actions across all sessions, with tamper-evident storage, is a future requirement.
- **Secret management**: Environment variables (`LINEAR_API_KEY`, `SLACK_WEBHOOK_URL`, etc.) are currently passed directly. Integration with secret managers or encrypted config is deferred.

## 8. Tech Stack

Language, runtime, and framework choices are TBD — see ADRs for decisions as they are made.

**Decided:**
- Terminal runtime: tmux (primary), iTerm2 (tab management)
- SCM integration: `gh` CLI, GitHub GraphQL API (for reviews)
- CI: GitHub Actions
- Security scanning: gitleaks

**To be decided (see ADRs):**
- Implementation language (TypeScript/Node.js vs Rust vs other)
- Monorepo structure and package manager
- Web dashboard framework
- Mobile app framework
- CLI framework
- Config validation library
- Test framework
- Real-time transport (SSE, WebSocket, etc.)
