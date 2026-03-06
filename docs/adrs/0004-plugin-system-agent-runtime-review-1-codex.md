# ADR-0004: Plugin System, Agent Contract & Runtime — Review Round 1 (Codex)

**Reviewer:** Codex
**Date:** 2026-03-06
**ADR Status:** Proposed
**Files Reviewed:**
- `docs/adrs/0004-plugin-system-agent-runtime.md`
- `docs/plans/2026-03-06-plugin-system-agent-runtime-design.md`
- `docs/prds/0001-agent-orchestrator.md` (v1.3)
- `docs/adrs/0001-session-lifecycle-engine.md`
- `docs/adrs/0002-implementation-language.md`
- `docs/adrs/0003-configuration-system.md`

---

## Summary

ADR-0004 addresses the co-design of the plugin framework, the Agent trait, and the Runtime trait. The central contribution is the **declarative plan pattern**: Agents produce typed step sequences (`LaunchPlan`), and the lifecycle engine feeds them to a Runtime via a single `execute_step()` method. The `RuntimeStep` enum is the decoupling seam. This is a well-motivated design that leverages Rust's type system effectively. The review identifies several gaps, mostly around error handling during plan execution, incomplete PRD coverage, and a hidden I/O dependency in `detect_activity()`.

---

## Strengths

1. **Decoupling via plans is the right abstraction.** The declarative plan pattern cleanly separates "what to run" (Agent) from "how to run it" (Runtime) and "when to run it" (lifecycle engine). The three alternatives considered (fat agent, session facade, hardcoded engine flow) are well-articulated and correctly rejected. This is the strongest decision in the ADR.

2. **Per-phase step enums exploit Rust's type system.** Using separate `RuntimeStep` and `ShutdownStep` enums to make invalid plan construction a compile error is exactly the kind of design ADR-0002 was chosen to enable. `LaunchPlan` cannot contain `ForceKill`; `ShutdownPlan` cannot contain `Create`.

3. **Prompt delivery modes as plan variations.** Encoding `inline`, `post-launch`, and `protocol` as different step sequences within `launch_plan()` — rather than as engine-level branches — is elegant. It means new delivery modes are additive to Agent implementations, not to the lifecycle engine.

4. **Idle promotion in the gatherer.** Keeping the `Ready -> Idle` timer-based promotion in the lifecycle engine's gather phase (per ADR-0001) rather than in the Agent trait is the correct separation. Agents report observed state; the engine applies time-based policy.

5. **Static factory functions are proportionate.** With ~3 plugins per slot at MVP, a `match` is simpler, more auditable, and provides exhaustive checking vs. a dynamic registry. The upgrade path to a registry is noted and non-breaking.

6. **Testing strategy is concrete.** The design doc includes actual test code for plans (unit, no I/O), activity detection (unit, no I/O), and tmux integration (requires tmux). The claim of testability is substantiated.

7. **Consistency with ADR-0002.** `Box<dyn Trait>`, `async-trait`, `Send + Sync` bounds, and `CommandRunner` are all used as specified. No deviations from the language ADR.

---

## Findings

### Critical

None.

### High

**H1. No plan execution error handling or partial rollback strategy.**

The ADR describes plan execution as "iterate steps, call `runtime.execute_step()`" (ADR line 141, design doc Section 4 lines 529-531). But what happens when step 2 of a 3-step plan fails? Specifically:

- For a `post-launch` plan `[Create, WaitForReady, SendMessage]`, if `WaitForReady` times out, the runtime has already created a session (step 1 succeeded). Who destroys it? The lifecycle engine? The caller?
- For a `protocol` plan `[Create, WaitForReady, SendProtocol]`, if `SendProtocol` returns `UnsupportedStep`, the session exists but was never given a prompt. It will appear alive but idle.

The design doc's session spawn pseudocode (Section 4, lines 525-532) shows a simple `for step in plan.steps` loop with no error handling. The ADR's Consequences section (line 163) mentions logging but not cleanup.

**Recommendation:** Define a plan execution contract: on step failure, the lifecycle engine calls `runtime.destroy(session_id)` to clean up any partially-created resources, and the session transitions to `errored`. Document this in the ADR's Decision section alongside the plan execution description.

**H2. `detect_activity()` has a hidden I/O dependency for JSONL-based agents.**

The ADR states plans are "testable without I/O" (line 155), and the Agent trait's `detect_activity(&self, output: &str) -> ActivityState` signature appears pure. However, the design doc (lines 369-376) shows the Claude Code implementation reads log files directly via `self.parse_jsonl_logs()`:

```rust
fn detect_activity(&self, terminal_output: &str) -> ActivityState {
    if let Some(state) = self.parse_jsonl_logs() {  // <-- filesystem I/O
        return state;
    }
    self.parse_terminal_output(terminal_output)
}
```

This means:
- The method signature suggests it is a pure function of `output`, but it is not.
- Unit testing the JSONL path requires either real log files on disk or injecting a log source.
- The `output` parameter is unused on the primary code path for the primary agent.

The ADR's Consequences section (line 166) acknowledges this tension but frames it as a minor trade-off. It is larger than stated: it undermines the testability claim for the most important agent implementation.

**Recommendation:** Either (a) change the signature to `detect_activity(&self, output: &str, log_content: Option<&str>) -> ActivityState`, letting the gatherer read the log file and pass its contents, keeping the Agent pure; or (b) inject a `LogReader` trait into `ClaudeCodeAgent` at construction so it can be stubbed in tests. Option (a) is simpler and keeps the gather phase responsible for all I/O, consistent with ADR-0001.

**H3. `WaitForReady` semantics are underspecified.**

The `WaitForReady { timeout: Duration }` step (ADR line 71, design doc line 171) is described as "Wait until the session is alive and responsive" for tmux, mapped to polling `tmux has-session`. But "alive" and "responsive" are different things:

- `tmux has-session` returns true as soon as the tmux session exists — the agent process may not have started or may still be initializing.
- For `post-launch` mode, if `SendMessage` fires immediately after `has-session` succeeds, the prompt may arrive before the agent is ready to accept input.

The design doc (line 221) maps `WaitForReady` to `tmux has-session` polling, which only checks session existence, not agent readiness. There is no mechanism to detect that the agent's interactive prompt is actually displayed.

**Recommendation:** Document the semantic gap between "runtime session exists" and "agent is ready for input." Consider: (a) adding an optional readiness check (e.g., wait for specific output pattern) as a step parameter, or (b) documenting that `WaitForReady` is best-effort and agents should tolerate early input (which Claude Code does handle, but may not be universal).

### Medium

**M1. PRD FR3 Runtime interface mismatch.**

The PRD (FR3, line 78) specifies the Runtime interface as: `create()`, `destroy()`, `sendMessage()`, `getOutput()`, `isAlive()`, optional `getMetrics()`, `getAttachInfo()`. The ADR replaces individual methods with `execute_step()`, which is a justified architectural evolution. However:

- `getMetrics()` is not addressed anywhere in the ADR or design doc. The design doc (line 237) defers it with "Token tracking via agent JSONL, not runtime." This should be explicitly called out as a PRD deviation — the PRD lists it as an optional Runtime method, and the ADR should state why it does not belong on the Runtime trait.
- `sendMessage()` in the PRD maps to both `SendMessage` and `SendBuffer` steps. The ADR should note this split and why.

**Recommendation:** Add a "PRD Interface Mapping" subsection showing how each PRD Runtime method maps to the ADR's design, including explicit notes on `getMetrics()` and the `sendMessage` split.

**M2. PRD FR1 Agent interface partial coverage.**

The PRD (FR1, line 41) specifies: `getLaunchCommand()`, `getEnvironment()`, `getActivityState()`, `isProcessRunning()`, `getSessionInfo()`, optional `getRestoreCommand()`, `postLaunchSetup()`, `setupWorkspaceHooks()`. The ADR maps most of these:

| PRD Method | ADR Mapping |
|---|---|
| `getLaunchCommand()` | `launch_plan()` (subsumes) |
| `getEnvironment()` | `launch_plan()` via `LaunchContext.env_extras` |
| `getActivityState()` | `detect_activity()` |
| `isProcessRunning()` | Moved to `Runtime::is_alive()` |
| `getSessionInfo()` | `parse_session_info()` |
| `getRestoreCommand()` | `restore_plan()` |
| `postLaunchSetup()` | Subsumed by multi-step `LaunchPlan` |
| `setupWorkspaceHooks()` | `workspace_hooks()` |

The mapping is reasonable but not documented. The ADR should make this explicit so reviewers can verify completeness.

**Recommendation:** Add a mapping table from PRD FR1 methods to ADR trait methods.

**M3. `SessionInfo` lacks fields present in PRD.**

The `SessionInfo` struct (design doc lines 384-389) has: `branch`, `pr_url`, `tokens_in`, `tokens_out`. The PRD's session metadata system (FR15, lines 316-320) also tracks `terminationReason` and the action journal. While `terminationReason` is a lifecycle engine concern (not agent-reported), the ADR should clarify which metadata is agent-sourced (via `parse_session_info()`) vs. engine-sourced. The current `SessionInfo` definition could be mistaken as the full session metadata schema.

**Recommendation:** Rename to `AgentSessionInfo` or add a doc comment clarifying this is agent-extracted metadata, not the full session record.

**M4. No `supports_step()` method on Runtime trait at MVP.**

The design doc (Section 6, lines 549-570) describes plan validation via `runtime.supports_step()` but defers the exact mechanism. The ADR (line 164) mentions this may be "needed" post-MVP with many runtimes. However, even at MVP, an agent configured with `protocol` delivery mode paired with a `tmux` runtime will fail at execution time (step 3 of 3), after session creation. Early validation would prevent this.

**Recommendation:** Add a `supported_steps(&self) -> &[&str]` or similar capability declaration to the Runtime trait at MVP. This is low-cost and prevents a class of configuration errors at session creation time rather than mid-execution.

**M5. `ShutdownStep` execution path undefined.**

`ShutdownPlan` uses a separate `ShutdownStep` enum (design doc lines 312-317) with `ForceKill` and `DestroyRuntime` variants. But `Runtime::execute_step()` accepts `RuntimeStep`, not `ShutdownStep`. How does the lifecycle engine execute a `ShutdownPlan`?

Options: (a) a second `execute_shutdown_step()` method on Runtime, (b) a separate execution function that maps `ShutdownStep` to existing Runtime methods (`ForceKill` -> `destroy()`, `DestroyRuntime` -> `destroy()`), (c) defer entirely.

Since `ShutdownPlan` is post-MVP, this is not blocking. But the ADR introduces the type without specifying its execution path, which may confuse implementers.

**Recommendation:** Add a brief note stating that `ShutdownPlan` execution will use direct Runtime method calls (e.g., `ForceKill` maps to `runtime.destroy()`) rather than `execute_step()`, since shutdown steps are a different vocabulary.

**M6. Session ID format and collision handling not specified.**

The ADR uses `session_id: &str` throughout but does not define the format or uniqueness guarantees. The design doc (line 226) mentions tmux session naming as `{sessionPrefix}-{issueId}`, but:

- What happens if two sessions are spawned for the same issue (e.g., retry after termination)?
- Are session IDs globally unique or scoped to a project?
- What prevents collision with manually-created tmux sessions?

ADR-0001 references session IDs but also does not define the format.

**Recommendation:** Define session ID format (e.g., `{sessionPrefix}-{issueId}-{attempt}` or UUID-based) and document uniqueness guarantees. This is a cross-cutting concern that affects Runtime, lifecycle engine, and metadata storage.

### Low

**L1. `ContinuePlan` and `RestorePlan` use the same step type as `LaunchPlan`.**

All three use `Vec<RuntimeStep>`. The ADR (line 124) justifies `ShutdownPlan` having its own step type because it includes destructive operations. But `RestorePlan` also has a `Create` step (design doc line 462), which means restore can create new sessions — is this always safe? If a session is being restored, the old runtime resource may still exist.

**Recommendation:** Document that `restore_plan()` assumes the previous runtime session has been destroyed (or is dead). The lifecycle engine should verify `!runtime.is_alive(session_id)` before executing a restore plan.

**L2. `attach_info()` is synchronous but other Runtime methods are async.**

`attach_info(&self, session_id: &str) -> Option<String>` (ADR line 89) is the only non-async method besides `meta()`. This is likely fine (it returns a formatted string, no I/O needed), but worth a comment in the trait definition explaining why it is sync.

**L3. The `process` runtime is deferred but listed in the factory.**

The design doc (line 63) includes `"process" => Ok(Box::new(ProcessRuntime::new(config)?))` in `create_runtime()`, but Section 2 (line 236) defers the `process` runtime to "fast-follow." If the factory arm exists but `ProcessRuntime` is not implemented, this should be a stub returning `PluginError::NotImplemented` (like the agent stubs).

**Recommendation:** Make the `process` runtime factory arm consistent with the agent stub pattern.

**L4. Design doc Section 6 references `supports_step()` not on the trait.**

The `validate_plan()` function (design doc lines 554-567) calls `runtime.supports_step(step)`, but this method is not part of the Runtime trait signature (Section 2, lines 128-157). This is noted as deferred but creates confusion since the pseudocode implies it exists.

**Recommendation:** Either add `supports_step()` to the trait or remove the pseudocode and replace with a prose description of the deferred plan.

---

## Consistency with Prior ADRs

| ADR | Assessment |
|-----|-----------|
| **ADR-0001 (Lifecycle Engine)** | Good integration. The three-phase poll loop (gather, evaluate, transition) is respected: `detect_activity()` and `parse_session_info()` serve the gather phase, `runtime.destroy()` serves entry actions. `Idle` promotion is correctly delegated to the gatherer. Session spawn is outside the poll loop, consistent with ADR-0001's scope. |
| **ADR-0002 (Implementation Language)** | Full alignment. `Box<dyn Trait>`, `async-trait`, `Send + Sync` bounds, `CommandRunner`, `tokio::process::Command` are all used as decided. The proof-of-concept spike for "one plugin slot trait with async methods" maps directly to this ADR's Runtime trait. |
| **ADR-0003 (Configuration System)** | Good. `AgentConfig` from the config schema flows into `LaunchContext.agent_config`. The `#[serde(flatten)]` extras field in `AgentConfig` is consumed by agent implementations (e.g., Claude Code reads `permissions`, `model`, `maxTurns`). The factory functions reference `&AgentConfig` and `&Config` consistent with ADR-0003's ownership model. One gap: ADR-0003 defers `AgentConfig.extra` validation to "FR1" — this ADR is FR1 but does not define what extras each agent validates. This is acceptable at MVP since only `claude-code` is fully implemented. |

---

## MVP Scope Assessment

The MVP scope is reasonable:
- `claude-code` agent with full implementation (JSONL detection, two delivery modes, restore, hooks)
- `tmux` runtime with full `RuntimeStep` support except `SendProtocol`
- Static factory functions for all 8 slots (only agent and runtime detailed here)
- Stubs for 5 other agents

**Potentially missing from MVP:**
- Plan execution error handling (H1) — this is essential for MVP since plan failures will occur during development and debugging
- Session ID format (M6) — needed before any sessions can be created

**Potentially over-scoped for MVP:**
- `ContinuePlan`, `RestorePlan`, `ShutdownPlan` trait methods — these are correctly marked as post-MVP with defaults, but defining the types in the ADR may encourage premature implementation. The current approach (define types, provide defaults, defer execution) is acceptable.

---

## Consequences Analysis

The stated consequences are accurate. Unstated consequences:

1. **The `RuntimeStep` enum is a versioning bottleneck.** Adding a new variant is a breaking change for all Runtime implementations (they must add a match arm or hit a compiler error). At MVP with 1-2 runtimes this is fine. Post-MVP, a `#[non_exhaustive]` attribute on the enum would allow additive changes without breaking downstream, at the cost of requiring a wildcard arm in all match statements.

2. **Agent implementations carry implicit file-system contracts.** Claude Code's `detect_activity()` reads `~/.claude/projects/.../logs/*.jsonl`. This path is not configurable and depends on Claude Code's internal log format, which may change without notice. The ADR should acknowledge this as a maintenance risk.

3. **The plan pattern creates a debugging indirection layer.** When an agent fails to launch, the developer must inspect both the plan (Agent output) and the execution (Runtime behavior). The ADR mentions logging (line 163) but does not specify a structured plan log format. Consider logging each plan as a structured JSON event before execution.

---

## Verdict

**Recommendation: Accept with revisions.** Address H1 (plan execution error handling) and H2 (detect_activity I/O) before accepting. H3 and M-level findings can be addressed during implementation.
