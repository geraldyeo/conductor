# ADR-0004: Plugin System, Agent Contract & Runtime — Review Round 2 (Codex)

**Reviewer:** Codex
**Date:** 2026-03-07
**ADR Status:** Proposed
**Round 1 Review:** `docs/adrs/0004-plugin-system-agent-runtime-review-1-codex.md`
**Files Reviewed:**
- `docs/adrs/0004-plugin-system-agent-runtime.md` (revised)
- `docs/plans/2026-03-06-plugin-system-agent-runtime-design.md` (revised)

---

## Purpose

Verify that round 1 findings (3 High, 6 Medium) have been adequately addressed, identify any new issues introduced by the revisions, and assess ADR readiness for acceptance.

---

## Round 1 Fix Verification

### H1. Plan execution error handling — RESOLVED

The ADR now includes a dedicated "Plan Execution Failure Semantics" section (ADR Section 6) and the design doc (Section 4) specifies the contract clearly: abort on failure, `runtime.destroy()` for cleanup if `Create` succeeded, transition to `errored` with step index and error in metadata, and structured logging of each step before/after execution. Three common failure modes are enumerated. This is thorough and sufficient.

### H2. `detect_activity()` hidden I/O — RESOLVED

The `GatherContext` struct replaces the bare `output: &str` parameter. The gatherer reads all I/O (terminal output via `runtime.get_output()`, auxiliary log from `agent.auxiliary_log_path()`), and Agent methods are pure functions over `GatherContext`. The design doc shows concrete code for both JSONL-based and terminal-based detection paths consuming the context without filesystem access. The testability claim is now substantiated — construct a `GatherContext` literal, call `detect_activity()`, assert the result. Well done.

### H3. `WaitForReady` semantics — RESOLVED

Both the ADR (Section 3, `WaitForReady` semantics paragraph) and design doc (Section 2, `WaitForReady` Semantics subsection) now document the semantic gap: `WaitForReady` checks session existence, not agent readiness. The requirement that agents tolerate early input is stated. Pattern-based readiness checks are explicitly deferred to post-MVP. This is adequate.

### M1. PRD FR3 Runtime interface mapping — RESOLVED

The ADR includes a "FR3 Runtime methods -> ADR trait methods" table covering all PRD methods. `getMetrics()` is explicitly addressed with rationale for deferral (token data is agent-sourced via JSONL, not runtime-sourced). The `sendMessage()` split into `SendMessage` and `SendBuffer` is noted.

### M2. PRD FR1 Agent interface mapping — RESOLVED

The ADR includes a "FR1 Agent methods -> ADR trait methods" table covering all 8 PRD methods with mapping rationale. `isProcessRunning()` is correctly noted as moved to `Runtime::is_alive()`. `postLaunchSetup()` subsumption by multi-step plans is explained.

### M3. `SessionInfo` scope — RESOLVED

Renamed to `AgentSessionInfo` in both the ADR and design doc, with a clear explanation that this is agent-extracted metadata and the lifecycle engine writes it alongside engine-sourced fields (`terminationReason`, action journal).

### M4. `supports_step()` missing from Runtime trait — RESOLVED

`supported_steps()` is now on the Runtime trait with a default implementation returning all step types. Plan validation via `validate_plan()` is documented in both ADR and design doc (Section 6). tmux overrides to exclude `"send_protocol"`. This catches mismatches at session creation.

### M5. `ShutdownStep` execution path — RESOLVED

The ADR (Section 5) and design doc (Section 3) now document the engine-level mapping: `SendMessage` -> `runtime.execute_step(RuntimeStep::SendMessage)`, `WaitForExit` -> poll `runtime.is_alive()`, `ForceKill`/`DestroyRuntime` -> `runtime.destroy()`. The asymmetry with `LaunchPlan` is acknowledged as intentional.

### M6. Session ID format — RESOLVED

Defined as `{sessionPrefix}-{issueId}-{attempt}` with concrete examples, uniqueness guarantees (lifecycle engine checks before spawning, increments attempt), and tmux-safe character validation (`[a-zA-Z0-9._-]`).

---

## Round 1 Low Findings (Not Required, Checking Status)

**L1. RestorePlan safety** — Not explicitly addressed but acceptable. The design doc's restore support section shows `--continue` as a new `Create` step, implying the old session is dead. Documenting the precondition (`!is_alive()` before restore) can happen during implementation.

**L2. `attach_info()` sync rationale** — Not addressed. Minor; the sync/async distinction is self-evident from the method's purpose.

**L3. `process` runtime factory stub** — Addressed. The design doc factory now returns `PluginError::NotImplemented` for `"process"`, consistent with agent stubs.

**L4. `supports_step()` pseudocode** — Resolved by M4 fix; the method is now on the trait.

---

## New Issues Introduced by Revisions

### Medium

**M1-new. `supported_steps()` uses string-based matching, not enum-based.**

`Runtime::supported_steps()` returns `&'static [&'static str]` and `RuntimeStep::type_name()` returns a string. Plan validation compares strings. This works but loses type safety — a typo in a string (e.g., `"send_mesage"`) would silently pass validation. Since `RuntimeStep` is already a well-defined enum, consider using `std::mem::discriminant` or a `RuntimeStepKind` enum for capability declaration, keeping the compile-time safety consistent with the rest of the design.

This is not blocking. The string-based approach is simpler and the set of step names is small and stable at MVP. Flag for implementation-time consideration.

**M2-new. `auxiliary_log_path()` returns a directory glob, not a file path.**

The ADR states Claude Code returns `Some(~/.claude/projects/.../logs/*.jsonl)` from `auxiliary_log_path()`. But the return type is `Option<PathBuf>`, and a glob pattern is not a valid `PathBuf`. The gatherer would need glob expansion logic. Either: (a) return the directory and have the gatherer find the latest file, (b) return an exact file path (requires the agent to resolve the glob), or (c) change the return type to support globs. This needs clarification before implementation.

### Low

**L1-new. `GatherContext.auxiliary_log` is `Option<String>` for potentially large JSONL files.**

The ADR's Consequences section notes the gatherer must read potentially large log files every tick and suggests "reading only the tail." But `GatherContext.auxiliary_log` is typed as `Option<String>` with no size bound. For implementation, consider documenting the expected tail-read strategy (last N bytes or lines) so that agent `detect_activity()` implementations can rely on recency rather than completeness. Not blocking — this is an implementation detail.

**L2-new. `ForceKill` vs `DestroyRuntime` distinction is unclear.**

Both `ShutdownStep::ForceKill` and `ShutdownStep::DestroyRuntime` map to `runtime.destroy()`. The semantic difference is not documented. If they are distinct operations (e.g., `ForceKill` = SIGKILL the process, `DestroyRuntime` = tear down the runtime session), this should be clarified. If they are synonyms, one should be removed. Post-MVP concern since `ShutdownPlan` is deferred.

---

## Consistency Check

### ADR vs Design Doc

The two documents are consistent after revisions. Key types (`GatherContext`, `AgentSessionInfo`, `RuntimeStep`, `ShutdownStep`, plan types), trait signatures, and PRD mapping tables match between documents. The design doc provides implementation detail (pseudocode, test code, module structure) that the ADR correctly summarizes without contradiction.

### ADR-0004 vs Prior ADRs

| ADR | Assessment |
|-----|-----------|
| **ADR-0001** | Consistent. Gather/evaluate/transition phases respected. `GatherContext` aligns with gather-phase I/O model. `Idle` promotion correctly in gatherer. Plan execution is outside the poll loop. |
| **ADR-0002** | Consistent. `Box<dyn Trait>`, `async-trait`, `Send + Sync`, `CommandRunner`, exhaustive matching all used as decided. |
| **ADR-0003** | Consistent. `AgentConfig` flows into `LaunchContext`. `PromptDelivery` added as a typed field on `AgentConfig`. |

### Round 1 Consequences Feedback

The three unstated consequences I flagged in round 1 are now addressed in the ADR's Consequences section:
1. `RuntimeStep` as versioning bottleneck — addressed with `#[non_exhaustive]` post-MVP note.
2. Implicit filesystem contract with Claude Code's JSONL path — acknowledged as maintenance risk.
3. Debugging indirection — addressed by structured step logging requirement.

---

## Verdict

**Recommendation: Accept.** All 3 High and 6 Medium findings from round 1 have been adequately addressed. The 2 new Medium findings (string-based step matching, glob path type) are implementation-time concerns that do not affect the architectural decisions. The ADR is internally consistent, consistent with the design doc, and consistent with prior ADRs. The design is sound and ready for acceptance.
