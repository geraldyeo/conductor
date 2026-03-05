# ADR Generation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create 7 foundational ADR files under `docs/adrs/` in hybrid format, plus a README index, and delete the old single-file `docs/ADR.md`.

**Architecture:** Each ADR is a standalone markdown file following a hybrid Nygard+MADR format (Status, Context, Considered Options, Decision, Consequences). A README.md in the same directory serves as the index with a status table.

**Tech Stack:** Markdown files only. No code.

---

### Task 1: Create README index

**Files:**
- Create: `docs/adrs/README.md`

**Step 1: Write the index file**

```markdown
# Architecture Decision Records

This directory contains the Architecture Decision Records (ADRs) for the Agent Orchestrator project.

## Format

Each ADR follows a hybrid format combining [Michael Nygard's original](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions) with [MADR's](https://adr.github.io/madr/) "Considered Options" section:

- **Status** -- Draft, Proposed, Accepted, Deprecated, or Superseded
- **Context** -- Problem and forces at play
- **Considered Options** -- Alternatives evaluated with trade-offs
- **Decision** -- What was chosen and why
- **Consequences** -- What becomes easier or harder

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-eight-slot-plugin-architecture.md) | Eight Slot Plugin Architecture | Draft |
| [0002](0002-workspace-isolation-git-worktrees.md) | Workspace Isolation via Git Worktrees | Draft |
| [0003](0003-terminal-multiplexing-tmux.md) | Terminal Multiplexing with tmux | Draft |
| [0004](0004-event-driven-lifecycle-polling.md) | Event-Driven Lifecycle Polling | Draft |
| [0005](0005-rule-based-automated-reactions.md) | Rule-Based Automated Reactions | Draft |
| [0006](0006-local-file-based-persistence.md) | Local File-Based Persistence | Draft |
| [0007](0007-implementation-language.md) | Implementation Language | Proposed |

## Layered Approach

ADRs are organized in layers. This first layer contains **foundational** decisions that gate downstream choices:

- **Layer 1 (this set):** Core architecture, isolation strategy, runtime, lifecycle, reactions, persistence, and implementation language.
- **Layer 2 (future):** CLI framework, config validation library, dashboard framework, mobile framework, test framework, real-time transport, monorepo structure. These depend on the implementation language decision (ADR-0007).
```

**Step 2: Commit**

```bash
git add docs/adrs/README.md
git commit -m "Add ADR index README with hybrid format description"
```

---

### Task 2: Create ADR-0001 (Eight Slot Plugin Architecture)

**Files:**
- Create: `docs/adrs/0001-eight-slot-plugin-architecture.md`

**Step 1: Write the ADR**

Reference material:
- `docs/PRD.md` FR3 (lines 51-66) for slot table and interface details
- `docs/plans/2026-03-05-adr-generation-design.md` ADR-0001 section

Content outline:
- **Status:** Draft
- **Context:** The orchestrator must support rapidly evolving AI agents (Claude Code, Codex, Aider, OpenCode, Gemini) and varied infrastructure (GitHub vs Linear, tmux vs Docker). Adding support for a new agent or tracker should not require modifying core orchestration logic. The system spans 8 distinct capability dimensions (see PRD FR3 table).
- **Considered Options:**
  1. **Monolithic core with built-in adapters** -- All agent/runtime/tracker logic lives in the core codebase behind if/switch statements. Simple to start, but every new integration touches core code. Testing requires mocking internals.
  2. **Strict slot-based plugin system** -- Define 8 named slots (Runtime, Agent, Workspace, Tracker, SCM, Notifier, Terminal, Lifecycle), each with a well-defined interface. Plugins register via a manifest (name, slot, description, version) and a factory function. Core only depends on slot interfaces, never on concrete implementations.
  3. **Generic middleware/hook pipeline** -- A single event bus where plugins register handlers for lifecycle events. Maximum flexibility, but no structure — hard to reason about which capabilities are present, no compile-time guarantees that required slots are filled.
- **Decision:** Option 2 — Strict slot-based plugin system. Each slot defines a clear interface contract. Plugins are discovered by slot name and instantiated via their factory function. The 8 slots are: Runtime, Agent, Workspace, Tracker, SCM, Notifier, Terminal, Lifecycle (core, not pluggable).
- **Consequences:**
  - *Positive:* Clear extension points for community contributions. New agents/runtimes/trackers can be added without touching core. Each slot's interface serves as documentation. Compile-time (or load-time) validation that required slots are filled.
  - *Negative:* More boilerplate per plugin (manifest + factory + interface implementation). Adding a 9th slot requires core changes. Slot interfaces must be designed carefully upfront — changing them is a breaking change for all plugins in that slot.

**Step 2: Commit**

```bash
git add docs/adrs/0001-eight-slot-plugin-architecture.md
git commit -m "Add ADR-0001: Eight Slot Plugin Architecture"
```

---

### Task 3: Create ADR-0002 (Workspace Isolation via Git Worktrees)

**Files:**
- Create: `docs/adrs/0002-workspace-isolation-git-worktrees.md`

**Step 1: Write the ADR**

Reference material:
- `docs/PRD.md` FR2 (lines 42-49) for workspace requirements
- `docs/plans/2026-03-05-adr-generation-design.md` ADR-0002 section

Content outline:
- **Status:** Draft
- **Context:** Running multiple AI agents in parallel requires isolated filesystems — each agent needs its own working directory and branch without conflicting with others. Traditional `git clone` duplicates the entire repository (slow for large repos, high disk usage). Branch switching within a single clone prevents parallelism entirely. The workspace strategy must also support symlinks for shared config files (`.env`, `.claude`) and post-create hooks (dependency installation).
- **Considered Options:**
  1. **Git worktree** -- Uses `git worktree add` to create lightweight checkouts that share the underlying `.git` object store. Fast creation (seconds), low disk overhead (only working tree files are duplicated). Native git support for branch isolation. Each worktree gets its own branch.
  2. **Full git clone per session** -- Complete repository copy per agent session. Maximum isolation (separate `.git` stores), but slow for large repos and high disk usage. Simpler mental model — each clone is fully independent.
  3. **Container-based isolation (Docker volumes)** -- Each agent runs in a Docker container with its own filesystem. Strongest isolation (OS-level), but heavy overhead, requires Docker daemon, and complicates the "human attach" workflow for debugging.
- **Decision:** Option 1 as default, with Option 2 as a supported fallback (the `clone` workspace plugin). Worktrees provide the best balance of speed, disk efficiency, and git-native isolation. The `clone` strategy exists for cases where worktree limitations apply (e.g., submodule edge cases, or users who prefer full isolation).
- **Consequences:**
  - *Positive:* Near-instant workspace creation. Low disk overhead for parallel sessions. Branch isolation is enforced by git itself (worktrees cannot share branches). Symlinks and post-create hooks work naturally.
  - *Negative:* Worktrees share reflog and git locks — potential contention under very high parallelism. Some git operations (e.g., `git gc`) affect all worktrees. Hash-based directory namespacing (`~/.agent-orchestrator/{sha256-12chars}-{projectId}/`) needed to prevent collisions between orchestrator instances.

**Step 2: Commit**

```bash
git add docs/adrs/0002-workspace-isolation-git-worktrees.md
git commit -m "Add ADR-0002: Workspace Isolation via Git Worktrees"
```

---

### Task 4: Create ADR-0003 (Terminal Multiplexing with tmux)

**Files:**
- Create: `docs/adrs/0003-terminal-multiplexing-tmux.md`

**Step 1: Write the ADR**

Reference material:
- `docs/PRD.md` FR3 Runtime slot (line 59) and FR5 send command (lines 106-110)
- `docs/plans/2026-03-05-adr-generation-design.md` ADR-0003 section

Content outline:
- **Status:** Draft
- **Context:** AI coding agents like Claude Code and Aider are CLI programs that read from stdin and write to stdout. The orchestrator needs to: (1) launch agents in persistent sessions that survive shell disconnects, (2) allow humans to "attach" to a running agent for debugging or manual intervention, (3) send messages (prompts, fix instructions) to running agents programmatically, and (4) capture agent output for activity detection.
- **Considered Options:**
  1. **tmux** -- Mature terminal multiplexer. Robust session management, survives disconnects, native attach/detach workflow. Supports programmatic interaction via `tmux send-keys` and output capture via `tmux capture-pane`. Widely available on Linux/macOS.
  2. **GNU Screen** -- Similar to tmux but older. Less scriptable, harder to capture output programmatically, fewer features for session management. Declining community adoption.
  3. **Direct process management (no multiplexer)** -- Spawn agents as child processes with piped stdin/stdout. Simpler architecture, no external dependency. But: no human attach capability, sessions die with the orchestrator, no disconnect survival.
  4. **Docker exec** -- Run agents in containers, interact via `docker exec`. Strong isolation, but heavy overhead, requires Docker daemon, and the attach UX is poor compared to tmux.
- **Decision:** Option 1 (tmux) as the primary local runtime, with Option 3 (direct process) as a lightweight fallback for headless/CI environments. tmux provides the best balance of programmability, persistence, and human-in-the-loop debugging. The `process` runtime exists for environments where tmux is unavailable or unnecessary.
- **Consequences:**
  - *Positive:* Human attach workflow is native (`tmux attach -t session`). Sessions survive orchestrator restarts. Programmatic message delivery via `tmux send-keys` with buffer support for large messages. Output capture for activity state detection.
  - *Negative:* tmux is a hard dependency for the default runtime (must be installed). tmux scripting has quirks (send-keys timing, buffer size limits). The `process` runtime fallback loses attach capability and disconnect survival.

**Step 2: Commit**

```bash
git add docs/adrs/0003-terminal-multiplexing-tmux.md
git commit -m "Add ADR-0003: Terminal Multiplexing with tmux"
```

---

### Task 5: Create ADR-0004 (Event-Driven Lifecycle Polling)

**Files:**
- Create: `docs/adrs/0004-event-driven-lifecycle-polling.md`

**Step 1: Write the ADR**

Reference material:
- `docs/PRD.md` Section 5 (Session Lifecycle, lines 253-294) and NFRs (lines 296-302)
- `docs/plans/2026-03-05-adr-generation-design.md` ADR-0004 section

Content outline:
- **Status:** Draft
- **Context:** The orchestrator must track the state of multiple concurrent sessions. State changes come from diverse async sources: agent activity (terminal output), CI systems (GitHub API), PR reviews (GraphQL), and runtime health (tmux). The coordination mechanism must handle 16 session statuses and 6 activity states (see PRD Section 5), detect transitions, and trigger reactions.
- **Considered Options:**
  1. **Webhook-driven (push)** -- Register webhooks for GitHub events (CI, PR, review). Agent state communicated via callback. Lowest latency for external events. But: local tmux sessions have no webhook surface, requires a publicly reachable endpoint (or ngrok), fragile under network issues, each source needs its own webhook handler.
  2. **Centralized polling loop** -- A single `LifecycleManager` periodically checks all sessions: runtime alive? agent activity state? PR/CI status? Review decision? Runs at configurable interval (default 30s). Re-entrancy guarded (skips if previous poll still running). Checks sessions concurrently.
  3. **Hybrid (webhooks + polling fallback)** -- Use webhooks where available (GitHub) for low-latency external events, poll for local state (tmux, agent activity). Most responsive but most complex — two code paths for every event source.
- **Decision:** Option 2 — Centralized polling. Simpler, works uniformly across all sources (local and remote), no public endpoint needed. The 30-second default interval is acceptable for CI/PR state changes. Can evolve to hybrid (Option 3) later without breaking changes — polling becomes the fallback.
- **Consequences:**
  - *Positive:* Single coordination path for all state sources. Resilient to network blips (retry on next poll). Works with local tmux sessions. No webhook infrastructure needed. Status determination logic is centralized and testable.
  - *Negative:* Up to 30-second delay detecting external events (CI pass, review comment). API rate limits become a concern with many sessions. Re-entrancy guard means a slow poll delays the next cycle.

**Step 2: Commit**

```bash
git add docs/adrs/0004-event-driven-lifecycle-polling.md
git commit -m "Add ADR-0004: Event-Driven Lifecycle Polling"
```

---

### Task 6: Create ADR-0005 (Rule-Based Automated Reactions)

**Files:**
- Create: `docs/adrs/0005-rule-based-automated-reactions.md`

**Step 1: Write the ADR**

Reference material:
- `docs/PRD.md` FR4 (lines 68-85) for reaction table and configuration
- `docs/plans/2026-03-05-adr-generation-design.md` ADR-0005 section

Content outline:
- **Status:** Draft
- **Context:** When the lifecycle manager detects state changes (CI failure, review comment, agent stuck), the system needs to respond. Some responses are mechanical (send CI failure logs to agent for a fix attempt), some require human judgment (approve and merge). Different teams have different tolerance levels — some want aggressive auto-fix, others want notification only. The system needs 9 distinct reaction types (see PRD FR4 table) with retry counting and escalation.
- **Considered Options:**
  1. **Hardcoded event handlers** -- Each event type has a fixed handler in the lifecycle manager. Simple, but not configurable. Every team gets the same behavior. Adding a new reaction requires code changes.
  2. **Declarative reaction rules** -- A `reactions` configuration maps event types to actions (`send-to-agent`, `notify`, `auto-merge`). Each rule has retry limits and escalation thresholds. Configurable globally with per-project overrides. The reaction tracker maintains per-session attempt counts.
  3. **User-defined scripts per event** -- Each event triggers a user-provided shell script or webhook. Maximum flexibility, but requires users to write and maintain scripts. Error handling and retry logic falls on the user. Hard to provide sensible defaults.
- **Decision:** Option 2 — Declarative reaction rules. Sensible defaults cover the 9 reaction types in the PRD. Per-project overrides allow teams to customize without forking. The three action types (`send-to-agent`, `notify`, `auto-merge`) cover the vast majority of use cases. Option 3 (user scripts) could be added later as a fourth action type.
- **Consequences:**
  - *Positive:* Decouples event detection from response — easy to reconfigure without code changes. Retry counting and escalation prevent infinite loops. Per-project overrides support diverse team workflows. Defaults work out of the box.
  - *Negative:* Reaction config can grow complex for advanced use cases. Action vocabulary is limited to 3 types (extensible later). Escalation thresholds are time-based, which may not suit all scenarios.

**Step 2: Commit**

```bash
git add docs/adrs/0005-rule-based-automated-reactions.md
git commit -m "Add ADR-0005: Rule-Based Automated Reactions"
```

---

### Task 7: Create ADR-0006 (Local File-Based Persistence)

**Files:**
- Create: `docs/adrs/0006-local-file-based-persistence.md`

**Step 1: Write the ADR**

Reference material:
- `docs/PRD.md` FR13 (lines 245-251) and NFRs persistence (line 300)
- `docs/plans/2026-03-05-adr-generation-design.md` ADR-0006 section

Content outline:
- **Status:** Draft
- **Context:** Session state — IDs, statuses, PR associations, branch names, metadata — must survive orchestrator restarts. Expected data volume is tens to low hundreds of sessions per orchestrator instance. The storage mechanism must support atomic writes (crash safety), concurrent reads, and be inspectable by humans and scripts. The orchestrator is a developer tool, not a production service — setup simplicity matters more than query power.
- **Considered Options:**
  1. **Flat key=value files** -- One metadata file per session in `~/.agent-orchestrator/`. Bash-compatible format (KEY=VALUE). Atomic writes via temp file + rename. Archive on delete (timestamped copies). Race-free session ID reservation via `O_EXCL` flag. Path traversal prevention via session ID validation.
  2. **SQLite** -- Single-file database. ACID transactions, SQL queries, concurrent access via WAL mode. Adds a binary dependency (libsqlite3 or bundled). Richer query capability for dashboard views.
  3. **External database (Postgres/Redis)** -- Full database server. Maximum query power and concurrent access. But: requires setup, running daemon, connection config. Overkill for a local developer tool.
- **Decision:** Option 1 — Flat key=value files. Zero dependencies. Human-readable and bash-scriptable (can `source` metadata files). Sufficient for the expected data volume. Hash-based directory namespacing (`~/.agent-orchestrator/{sha256-12chars}-{projectId}/`) allows multiple orchestrator instances to coexist.
- **Consequences:**
  - *Positive:* Zero setup, zero dependencies. Files are human-readable and scriptable. Atomic writes prevent corruption. Archive-on-delete provides audit trail. Works on any OS with a filesystem.
  - *Negative:* No complex queries — listing sessions requires scanning files. No transactions across multiple files. Performance degrades at very high session counts (unlikely in practice). Dashboard must aggregate data from individual files.

**Step 2: Commit**

```bash
git add docs/adrs/0006-local-file-based-persistence.md
git commit -m "Add ADR-0006: Local File-Based Persistence"
```

---

### Task 8: Create ADR-0007 (Implementation Language)

**Files:**
- Create: `docs/adrs/0007-implementation-language.md`

**Step 1: Write the ADR**

Reference material:
- `docs/PRD.md` Section 7 (lines 304-322) for TBD items
- `docs/plans/2026-03-05-adr-generation-design.md` ADR-0007 section

Content outline:
- **Status:** Proposed
- **Context:** The orchestrator is a CLI-first tool that manages tmux sessions, git worktrees, and shell processes. It needs to: (1) ship as an easily distributable binary or package, (2) handle concurrent session polling efficiently, (3) parse YAML configuration, (4) interact with GitHub APIs (REST + GraphQL), and (5) serve a web dashboard. The choice of implementation language gates all downstream tech decisions: CLI framework, config library, async runtime, web framework, test framework, and monorepo structure.
- **Considered Options:**
  1. **Rust** -- Compiles to a single static binary. No runtime dependency on target machine. Strong concurrency via tokio async runtime. Excellent CLI ecosystem (clap). serde for config serialization (YAML, JSON). Traits map naturally to the 8-slot plugin architecture. Strong type system catches errors at compile time. Steeper learning curve, longer compile times, smaller contributor pool. Dashboard would be a separate process (e.g., Axum serving a SPA or htmx). Mobile app would be a fully separate codebase.
  2. **TypeScript/Node.js** -- Fastest iteration speed, largest ecosystem. Same language for CLI + dashboard (Next.js) + potentially mobile (React Native). Commander.js for CLI, Zod for config validation. Requires Node.js runtime on target machine. The upstream reference implementation (ComposioHQ/agent-orchestrator) uses this stack. Weaker type guarantees at runtime despite TypeScript's compile-time checks.
  3. **Go** -- Single binary like Rust, simpler concurrency model (goroutines), strong CLI ecosystem (cobra). Common for DevOps/infrastructure tooling. Less expressive type system than Rust (no generics until recently, no sum types). Larger contributor pool than Rust for DevOps tools. Dashboard would be a separate frontend build.
- **Decision:** Pending. Leaning Rust.
- **Consequences (if Rust):**
  - *Positive:* Single binary distribution — no runtime dependencies for end users. Excellent performance for polling and process management. Traits provide a natural, type-safe plugin system. Memory safety without GC pauses. Cross-compilation for Linux/macOS/Windows.
  - *Negative:* Longer compile times (mitigated by cargo workspaces + incremental compilation). Smaller contributor pool. Dashboard needs a separate frontend build pipeline (can't share code with CLI). Mobile app is a fully separate codebase. Steeper onboarding for new contributors.
  - *Downstream decisions unlocked:* CLI framework (clap), config (serde + serde_yaml), async runtime (tokio), test framework (built-in + cargo-nextest), web framework for dashboard API (axum or actix-web), plugin system (traits + dynamic dispatch or compile-time generics).

**Step 2: Commit**

```bash
git add docs/adrs/0007-implementation-language.md
git commit -m "Add ADR-0007: Implementation Language (Proposed)"
```

---

### Task 9: Delete old ADR.md and final commit

**Files:**
- Delete: `docs/ADR.md`
- Modify: `docs/README.md` -- Update to point to `docs/adrs/` instead of `./ADR.md`

**Step 1: Delete old ADR file**

```bash
git rm docs/ADR.md
```

**Step 2: Update docs/README.md**

Change the ADR link from `./ADR.md` to `./adrs/`:

```markdown
# Agent Orchestrator Documentation

This folder contains the documentation for the Agent Orchestrator project.

## Documents

- [Product Requirements Document (PRD)](./PRD.md) - Functional and non-functional requirements.
- [Architecture Decision Records (ADRs)](./adrs/) - Key technical decisions and rationales.
- [Design Plans](./plans/) - Design documents and implementation plans.
```

**Step 3: Commit**

```bash
git add docs/ADR.md docs/README.md
git commit -m "Remove old ADR.md, update docs index to point to docs/adrs/"
```
