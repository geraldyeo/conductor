# Holistic Cross-Cutting Review: ADR-0001 through ADR-0008

**Date:** 2026-03-07
**Reviewers:** Gemini CLI, Codex CLI, Claude (synthesizer)
**Scope:** All 8 MVP ADRs reviewed as an integrated whole
**Resolution:** All 6 High findings addressed — see changes below

---

## High Findings (must address before implementation)

### H1. IPC Serialization Blocks the Poll Loop (ADRs 0001, 0007)

**Consensus: Gemini + Claude**

`ao send` includes a busy-wait loop (up to 600s timeout) for activity detection. Because ADR-0007's concurrency model processes IPC requests within the poll loop task (between ticks), a single slow `ao send` blocks the lifecycle engine for ALL sessions. This fundamentally undermines parallel orchestration.

**Recommendation:** The `ao send` delivery flow (busy-wait + deliver + verify) must be decoupled from the poll loop. Options: (a) spawn a per-request tokio task for `Send` requests that holds its own Agent + Runtime references, (b) process `Send` as an async job with status polling, or (c) move activity detection into the gather phase and queue sends for delivery during transition. The key invariant is: no IPC request should block the poll loop for more than the time to enqueue it.

---

### H2. Kill Intent Is Not Persisted (ADRs 0001, 0005, 0007)

**Source: Codex**

`ao session kill` sets `manualKill = true` as an in-memory flag on `PollContext`. If the orchestrator crashes or restarts before the next poll tick processes it, the kill request is silently lost. `SessionMetadata` has no durable "kill requested" field.

**Recommendation:** Add a `KILL_REQUESTED=true` field to `SessionMetadata` (or a dedicated flag file in the session directory). The gatherer checks this field on restart and populates `PollContext.manualKill`. This costs one extra metadata write per kill but ensures durability across restarts.

---

### H3. Budget Enforcement Config Gap (ADRs 0001, 0003)

**Consensus: Gemini + Claude**

ADR-0001 explicitly includes budget enforcement in MVP scope (global edge at precedence 1: `budgetExceeded`). ADR-0004 confirms `parse_session_info()` runs every tick for fresh token counts. But ADR-0003 defers `maxSessionTokens` and `maxSessionWallClockMs` to post-MVP as `Option<Value>` — meaning the config system cannot load or validate these fields at MVP. The engine has the edge but no way to evaluate it.

**Recommendation:** Either (a) promote `maxSessionTokens` and `maxSessionWallClockMs` to typed `Option<u64>` fields in the MVP config schema (they're already in the PRD), or (b) explicitly remove the `budgetExceeded` global edge from MVP scope and document it as post-MVP. Option (a) is preferred — the fields are simple, the guard is `ctx.budget_exceeded` which is trivially computed from session metadata token counts vs config limits.

---

### H4. Tracker-Terminal Cleanup Path Conflicts with CLI Cleanup (ADRs 0001, 0005, 0007)

**Source: Codex**

Two distinct paths lead to session termination for tracker-terminal issues:
- **Lifecycle engine path:** `trackerState == Terminal` → `cleanup` global edge (precedence 28) → `TerminationReason::TrackerTerminal` → `git branch -d` (safe delete)
- **CLI path:** `ao session cleanup` → sets `manualKill = true` → `killed` status (precedence 0) → `TerminationReason::ManualKill` → `git branch -D` (force delete)

The CLI path produces the wrong termination reason and wrong branch deletion behavior. Work that was completed but whose issue was closed should get safe `-d` deletion, not force `-D`.

**Recommendation:** `ao session cleanup` should set a `trackerCleanup` flag (not `manualKill`) that the lifecycle engine processes via the tracker-terminal edge, preserving correct termination reason and branch deletion semantics. Alternatively, `ao session cleanup` could directly transition sessions to `cleanup` status via IPC rather than piggy-backing on `manualKill`.

---

### H5. Cross-Plugin Capability Validation Is Underspecified (ADRs 0003, 0004)

**Consensus: Codex + Claude**

A user can configure `promptDelivery: protocol` with `runtime: tmux`, which is incompatible — tmux returns `UnsupportedStep` for `SendProtocol`. ADR-0004 defines `supported_steps()` on Runtime and mentions "plan validation at session creation," but this validation is not wired into config validation (ADR-0003) or startup validation (ADR-0007).

**Recommendation:** Add startup-time cross-plugin validation: after constructing plugins per project, check that `agent.launch_plan(mock_context)` produces only steps in `runtime.supported_steps()`. Alternatively, validate `prompt_delivery` against runtime capabilities in the config validation pass. This catches misconfiguration at startup rather than at first spawn.

---

### H6. Deleted Issue Causes Immediate Workspace Destruction (ADRs 0001, 0006)

**Source: Gemini**

ADR-0006 treats `TrackerError::NotFound` (deleted issue) as `TrackerState::Terminal`. This triggers the `cleanup` global edge, destroying the runtime and workspace — including any uncommitted work. An accidental issue deletion on GitHub leads to irreversible loss of an agent's in-progress work.

**Recommendation:** Either (a) introduce a `TrackerState::Missing` variant that triggers a warning + notification instead of immediate cleanup, giving the human time to recreate the issue, or (b) add a configurable `deletedIssuePolicy` (already listed as deferred in ADR-0006) with values `terminal` (current) vs `warn` (notify but don't kill). At minimum, the workspace should be archived before destruction so work can be recovered.

*Note: ADR-0005's archive-on-delete partially mitigates this — the metadata is preserved. But the git worktree (with uncommitted changes) is destroyed by `git worktree remove --force`.*

---

## Medium Findings (should address before implementation)

### M1. Spawn Sequence Numbering Drift (ADRs 0005, 0007, 0008) — Consensus: Codex + Claude

ADR-0005 defines a 10-step spawn sequence. ADR-0008 inserts prompt composition as "step 6" but the numbering doesn't align with ADR-0005's original steps (which had afterCreate at step 5, beforeRun at step 8). ADR-0007 references ADR-0005's "10-step session creation sequence." The steps need a single canonical numbering.

**Recommendation:** Create a single authoritative spawn sequence table in one ADR (probably ADR-0005 or a new integration doc) and have other ADRs reference it by step name, not step number.

---

### M2. PollContext Type Contract Drift (ADRs 0001, 0006) — Consensus: Codex + Claude

ADR-0001 defines `PollContext.trackerState` as a conceptual field (implied string). ADR-0006 replaces it with `TrackerState` enum and says "the enum lives in `packages/core/src/tracker/mod.rs`." But ADR-0001's guards reference `ctx.trackerState === "terminal"` (string comparison). The actual type and module location need to be reconciled.

**Recommendation:** Normalize: `TrackerState` lives in `packages/core/src/types/` alongside `SessionStatus` and `TerminationReason`. ADR-0001 guards use `ctx.tracker_state == TrackerState::Terminal`.

---

### M3. Cross-Cutting Error Policy Is Inconsistent (ADRs 0001, 0004, 0006, 0007) — Consensus: Codex + Claude

Error handling varies across subsystems without a unified policy:
- Tracker API failure → default to active (safe, convergent)
- Hook failure in `beforeRemove` → warn, continue
- LaunchPlan step failure → abort, destroy, errored
- IPC connection failure → exit code 4
- `add_comment()` failure → warn, non-blocking

Each choice may be individually reasonable, but there's no overarching error taxonomy. Implementers will make inconsistent local decisions for unlisted scenarios.

**Recommendation:** Define a short error handling policy doc (or a section in a cross-cutting ADR) with three categories: (a) convergent errors (default-safe, retry next tick), (b) fatal errors (abort operation, transition to errored), (c) advisory errors (log + continue). Each ADR's error paths should be classified.

---

### M4. Observability Has No Shared Schema (ADRs 0001-0008) — Consensus: Codex + Claude

`tracing` is chosen (ADR-0002), journal entries use JSONL (ADR-0005), IPC has structured JSON (ADR-0007), but there's no shared correlation ID or event schema across these layers. Debugging a failed spawn requires correlating CLI logs, IPC traces, plan execution logs, hook output, and journal entries — with no common session/request ID threading.

**Recommendation:** Define a `trace_id` (or reuse `session_id`) that threads through IPC requests → poll ticks → plan steps → hook runs → journal entries. Add structured `tracing` spans per session per tick.

---

### M5. `ao send` Verification Race Condition (ADRs 0004, 0007) — Source: Gemini

`ao send` verifies delivery by checking if activity state transitions to `active`. For fast-completing commands, the agent may cycle `Ready → Active → Ready` between detection intervals, causing a false delivery failure report.

**Recommendation:** Verification should check for *any* activity state change (not just transition to `active`), or use a sequence number / timestamp on the activity state to detect that the agent processed input.

---

### M6. Symlink Name Validation Gap (ADRs 0003, 0005) — Source: Gemini

Symlink *targets* are validated against repo-escape, but symlink *names* (the destination path within the worktree) are not. A config like `symlinks: ["../../../etc/foo"]` would create a symlink outside the worktree directory.

**Recommendation:** Validate that the symlink destination (worktree-relative) resolves within the worktree root, using the same `canonicalize()` + prefix check pattern.

---

### M7. `WaitForReady` Is Insufficient for Post-Launch Delivery (ADR-0004) — Source: Gemini

`WaitForReady` verifies the runtime session exists (e.g., `tmux has-session`), not that the agent process is ready for input. Post-launch plans send the prompt immediately after `WaitForReady`, but the agent may still be initializing. ADR-0004 acknowledges this ("agents must tolerate early input delivery") but this is fragile.

**Recommendation:** For MVP (Claude Code + tmux), this is acceptable since tmux buffers input. Document this as a known limitation and add a `WaitForOutput { pattern, timeout }` step type post-MVP for agents that require readiness confirmation.

---

### M8. End-to-End Integration Test Ownership (ADRs 0001-0008) — Source: Codex

Individual ADRs reference "testing strategy" in their design docs, but critical cross-ADR journeys (spawn → workspace → prompt → launch → poll → transition → cleanup) have no explicit test ownership.

**Recommendation:** Define 5-7 critical integration test scenarios that span multiple ADRs, assign each to a specific crate/module, and include them in the implementation plan.

---

### M9. Global Socket Prevents Multi-Instance (ADRs 0005, 0007) — Source: Gemini

`~/.agent-orchestrator/orchestrator.sock` is a single global socket, but ADR-0005 uses hash-based directories to support multiple project configs. If a user needs two orchestrator instances (different configs), the fixed socket path is a collision.

**Recommendation:** This is acceptable for MVP (one orchestrator manages all projects in one config). Document as a known limitation. Post-MVP, the socket could move to a per-config-hash location or the orchestrator could support multiple configs natively.

---

## Low Findings (minor, no action required)

| # | Finding | ADRs | Source |
|---|---------|------|--------|
| L1 | `TerminationReason` has no success variant — merged/done sessions have `None` | 0005 | Gemini |
| L2 | `AO_SESSION`/`AO_DATA_DIR` env var injection point is ambiguous — engine sets at step 9 but agent produces `Create` step (with env map) at step 8 | 0004, 0005 | Codex |
| L3 | `ao status` shows stale data (reads files directly, may lag one tick behind orchestrator state) | 0001, 0007 | Gemini, Codex |
| L4 | Status/termination terminology could benefit from a canonical glossary table | 0001, 0005, 0007 | Codex |
| L5 | MVP scope narrative could be tightened — ADR-0008 defines orchestrator prompt now but FR13 is deferred | 0007, 0008 | Codex |
| L6 | External dependency version policy (Claude JSONL paths, `gh --json`, `git worktree --porcelain`) is per-ADR, not unified | 0004, 0005, 0006 | Codex |
| L7 | `ao session kill` UX: 5s CLI poll vs 30s tick means most kills show "scheduled" not "confirmed" | 0001, 0007 | Gemini |

---

## Reviewer Disagreements

### D1. IPC Security (UDS Permissions)

- **Codex** rated the lack of explicit socket permission model as **High**, citing hooks and destructive actions reachable via IPC.
- **Gemini** and **Claude** consider this **Low** for MVP — the orchestrator is a local developer tool, UDS inherits filesystem user permissions, and the tool runs as the current user. Standard Unix permission model is sufficient. Post-MVP hardening (for shared servers) is a separate concern.

**Resolution:** Accept as-is for MVP. Add a note to ADR-0007 that socket permissions should use `0o700` mode for defense-in-depth.

### D2. Deleted Issue → Terminal Severity

- **Gemini** rated this **High** (irreversible workspace destruction).
- **Claude** rates this **Medium** — ADR-0005's archive-on-delete preserves metadata, and uncommitted git changes in the worktree are the real risk. However, agents typically commit frequently, and the scenario (accidental issue deletion) is rare.

**Resolution:** Elevated to **High** in this report because the worktree destruction IS irreversible for uncommitted work, regardless of metadata archival.

---

## Overall Assessment

The 8 ADRs form a **well-structured, internally consistent architecture**. The key design patterns — declarative LaunchPlans, pure guard functions, gather/evaluate/transition phases, DataPaths as path arithmetic, and the engine-as-coordinator model — compose cleanly across ADRs.

**Strengths:**
- Excellent decoupling between Agent, Runtime, Workspace, Tracker, and Prompt subsystems
- The `RuntimeStep` enum as the shared vocabulary between Agent and Runtime is elegant
- Config forward-compatibility (`Option<Value>`, `#[serde(flatten)]`) is well thought out
- Crash recovery via re-poll (no event sourcing) is pragmatic and correct

**Primary risks:**
1. The IPC-blocks-poll-loop issue (H1) is the most critical — it's a structural flaw that undermines the core value proposition of parallel orchestration
2. Budget config gap (H3) and cleanup path conflict (H4) are inconsistencies that will surface as bugs during implementation
3. Kill intent durability (H2) is a correctness issue for the most common user-facing operation

**Recommendation:** Address the 6 High findings before beginning implementation. The Medium findings can be addressed during implementation as they represent specification gaps rather than architectural flaws.
