# ADR-0004 Review — Round 1 (Gemini)

**ADR version reviewed:** 0004-plugin-system-agent-runtime.md as of 2026-03-06 (status: Proposed)
**Design doc reviewed:** docs/plans/2026-03-06-plugin-system-agent-runtime-design.md

## Strengths

1. **Declarative plan pattern is well-motivated and elegant.** The `LaunchPlan` / `RuntimeStep` intermediary cleanly decouples Agent from Runtime. The demonstration that all three prompt delivery modes (inline, post-launch, protocol) reduce to different step sequences executed by the same engine loop is convincing. This is the design's strongest contribution — it moves delivery-mode variation into the Agent (where domain knowledge lives) and out of the lifecycle engine (where it would accrete `match` branches over time).

2. **Per-phase step enums leverage Rust's type system.** The decision to give `ShutdownPlan` its own `ShutdownStep` enum (with `ForceKill` and `DestroyRuntime`) while `LaunchPlan` uses `RuntimeStep` (which cannot express those destructive operations) is exactly the kind of compile-time safety ADR-0002 was chosen for. This prevents an entire class of invalid plan construction bugs.

3. **Idle promotion stays in the gatherer.** The explicit decision that `detect_activity()` never returns `Idle` — the lifecycle engine's gatherer promotes `Ready` to `Idle` based on elapsed time — is consistent with ADR-0001's principle that timer-based triggers are handled in the gather phase. This keeps the Agent trait stateless with respect to time, which simplifies testing and avoids clock-dependency in agent implementations.

4. **Testability claims are substantiated.** The design doc's testing strategy (Section 7) includes concrete test examples for launch plans, activity detection, and runtime integration. The plan-based pattern genuinely enables no-I/O unit tests for agent logic — construct a `LaunchContext`, call `launch_plan()`, assert on steps. This is a real advantage over the "fat Agent" alternative.

5. **Clean integration with ADR-0001.** The lifecycle engine integration section (design doc Section 4) maps Agent/Runtime calls to the three poll-loop phases (gather, evaluate, transition) without ambiguity. The session spawn sequence is linear and comprehensible.

6. **Honest consequences section.** The ADR acknowledges real trade-offs: the indirection cost when debugging launch failures, the `detect_activity(&str)` parameter being unused for JSONL-based agents, and the conceptual overhead of multiple step enums. These are genuine negatives, not hand-waved.

7. **Static factory functions are the right call for MVP.** With all plugins compiled in and ~3 per slot, a `match` is simpler, exhaustive, and more auditable than a registry. The migration path to a registry is acknowledged without over-designing it now.

## Findings

### Critical

None.

### High

**H1. `detect_activity()` has a hidden I/O dependency that contradicts the "no I/O in Agent" claim.**

The design doc (lines 369-376) shows the Claude Code implementation of `detect_activity()` calling `self.parse_jsonl_logs()` internally, which reads log files from disk (`~/.claude/projects/.../logs/*.jsonl`). The method signature takes `&self` and `&str` (terminal output), suggesting it is a pure parsing function. However, the primary detection path for the default agent performs filesystem I/O.

This creates three problems:
- The "Agent plans are testable without I/O" claim (ADR line 155, design doc Section 7) is only half true. Plans are pure, but `detect_activity()` is not.
- The gather phase (ADR-0001) already calls `runtime.get_output()` to obtain terminal output and passes it to `detect_activity()`. If the agent also reads log files independently, the gatherer has no visibility into what I/O the agent performs, breaking the sequential-per-session I/O model.
- Testing `detect_activity()` for Claude Code requires either real log files on disk or refactoring to inject a log reader.

**Recommendation:** Make the I/O explicit. Either: (a) extend the Agent trait to include a `gather_activity_inputs()` async method that returns agent-specific data (JSONL content, protocol response, etc.), which the gatherer calls and then passes to a pure `detect_activity()` — or (b) accept that `detect_activity()` may perform I/O for some agents, change the method to `async`, and document that the gather phase calls it as an async operation. Option (a) preserves purity for the detection logic; option (b) is simpler but loses the pure-function property.

**H2. `ShutdownStep` execution path is undefined — no `Runtime` method handles it.**

The ADR (lines 121-124) and design doc (lines 311-317) define `ShutdownStep` with variants `SendMessage`, `WaitForExit`, `ForceKill`, and `DestroyRuntime`. However, `Runtime::execute_step()` takes `&RuntimeStep`, not `&ShutdownStep`. There is no method on the Runtime trait that accepts `ShutdownStep`.

The design doc marks `ShutdownPlan` execution as deferred (Section 9, line 650), but the trait methods `shutdown_plan()` and `destroy()` are both defined now. When post-MVP implementation arrives, the Runtime trait will need either: (a) a new `execute_shutdown_step(&self, session_id: &str, step: &ShutdownStep)` method (breaking trait change), or (b) `ShutdownStep` variants mapped to existing Runtime methods by the lifecycle engine (reintroducing the engine-level `match` the plan pattern was designed to avoid).

**Recommendation:** Acknowledge this gap explicitly. Either: (a) add `execute_shutdown_step()` to the Runtime trait now with a default implementation that returns `UnsupportedStep` (non-breaking, forward-compatible), or (b) redesign `ShutdownStep` to be a subset of `RuntimeStep` plus lifecycle-engine-level operations (`ForceKill` maps to `runtime.destroy()`, not a step). Option (b) may be cleaner since `ForceKill` and `DestroyRuntime` are lifecycle-engine actions, not runtime-step actions.

**H3. Plan step failure semantics are unspecified.**

The ADR (line 137) and design doc (Section 4, lines 528-531) show the session spawn loop as:

```
for step in plan.steps:
    runtime.execute_step(session_id, step)
```

But what happens when a step in the middle of a multi-step plan fails? For example, in the post-launch plan `[Create, WaitForReady, SendMessage]`:
- If `WaitForReady` times out, should `SendMessage` still execute? (Almost certainly not.)
- Should the engine attempt cleanup (destroy the runtime created by the `Create` step)?
- What session status should be set? `errored`? `spawning` (and let the next poll cycle detect it)?

The design doc's testing section (line 616) tests the happy path only. The failure path — partial plan execution — is not addressed anywhere.

**Recommendation:** Specify failure semantics for plan execution: (a) first failing step aborts the plan, (b) the lifecycle engine destroys the runtime on plan failure (or transitions to `errored`), (c) the error from the failing step is logged with the step index for debugging. This is important for MVP since `WaitForReady` timeouts will be a common failure mode.

### Medium

**M1. `supports_step()` is referenced but not defined on the Runtime trait.**

The design doc (Section 6, lines 556-567) shows a `validate_plan()` function that calls `runtime.supports_step(step)`. The text acknowledges this method does not exist yet ("deferred to implementation"). However, the ADR's Consequences section (line 164) says "a `supports_step()` capability declaration may be needed" post-MVP.

Without `supports_step()`, plan validation cannot run at session creation. The only way to discover an unsupported step is to execute the plan and get `RuntimeError::UnsupportedStep` at runtime. For MVP with only tmux (which supports everything except `SendProtocol`) this is acceptable, but it means the first user to configure a `protocol`-mode agent with the tmux runtime will get a runtime error instead of a startup validation error.

**Recommendation:** Add `fn supported_steps(&self) -> &[&str]` or `fn supports_step(&self, step: &RuntimeStep) -> bool` to the Runtime trait with a default returning `true`. This is a minor addition that enables early validation without requiring all implementations to override it.

**M2. `SessionInfo` extraction assumes terminal output, but token tracking may come from JSONL.**

The Agent trait's `parse_session_info(&self, output: &str)` (ADR line 103, design doc line 258) takes terminal output. `SessionInfo` includes `tokens_in` and `tokens_out` (design doc lines 385-389). For Claude Code, token counts come from JSONL logs, not terminal output — the same I/O concern as H1 but for a different method.

If `parse_session_info()` is intended to be called with terminal output from `runtime.get_output()`, Claude Code's implementation will need to perform its own file I/O to extract token counts, making the `output` parameter misleading for the primary agent.

**Recommendation:** Either rename the parameter to clarify it is a hint (not the sole input), or restructure so that `detect_activity()` and `parse_session_info()` share an agent-specific gathered context (as suggested in H1).

**M3. `LaunchContext` does not include `prompt_delivery` mode.**

The `LaunchContext` struct (design doc lines 327-333) contains `prompt`, `workspace_path`, `session_id`, `agent_config`, and `env_extras`. The PRD (FR1, lines 42-45) specifies that prompt delivery mode is a per-agent property (`promptDelivery`). ADR-0003's `AgentConfig` does not include a `prompt_delivery` field (it would be in the `extra` HashMap via `serde(flatten)`).

The Claude Code implementation (design doc lines 397-452) shows two different `launch_plan()` implementations — one for inline, one for post-launch — but does not show how the agent decides which to use. The comment says "when agent_config indicates post-launch mode" but `AgentConfig` has no typed field for this.

**Recommendation:** Either: (a) add `pub prompt_delivery: Option<String>` to `AgentConfig` (it is a cross-agent concern, not agent-specific), or (b) document that agents read `prompt_delivery` from `agent_config.extra` and show the lookup in the Claude Code implementation. Option (a) is cleaner since prompt delivery mode is defined in the PRD as a standardized property, not an agent-specific extra.

**M4. No `getMetrics()` equivalent on the Agent trait for token tracking.**

The PRD (FR3, line 78) lists `getMetrics()` as an optional Runtime interface method. The PRD (FR15, line 318) requires per-session token tracking. The design doc defers `getMetrics()` (Section 2 deferred table, line 237) with the note "Token tracking via agent JSONL, not runtime."

However, there is no corresponding method on the Agent trait either. `parse_session_info()` returns `SessionInfo` with `tokens_in/tokens_out`, but these are extracted during the gather phase alongside activity detection. If the lifecycle engine wants to check token usage against `maxSessionTokens` (for the budget-exceeded global edge in ADR-0001), it needs token counts available in `PollContext`. The pipeline is: `runtime.get_output()` -> `agent.parse_session_info(output)` -> extract tokens -> populate `PollContext`.

This pipeline works but relies on `parse_session_info()` being called every poll cycle, not just when output changes. This should be documented.

**Recommendation:** Document in the lifecycle integration section that `parse_session_info()` is called every poll tick (not just on output changes) so that token counts are fresh for budget evaluation.

**M5. `WorkspaceHook` type is used but never defined.**

The Agent trait (design doc line 278, ADR line 109) returns `Vec<WorkspaceHook>`. The Claude Code implementation (design doc lines 474-478) uses `WorkspaceHook::PostToolUse { script: String }`. But `WorkspaceHook` is never defined as an enum or struct in either document. The PRD (FR2, lines 61-66) specifies four hook types: `afterCreate`, `beforeRun`, `afterRun`, `beforeRemove`. The design doc's hook is `PostToolUse`, which is agent-specific (Claude Code's tool-use callback), not one of the four PRD hooks.

The relationship between the PRD's four workspace lifecycle hooks (which are shell commands in config) and the agent's `workspace_hooks()` method (which returns programmatic hooks) is unclear.

**Recommendation:** Define the `WorkspaceHook` enum explicitly. Clarify whether it covers the four PRD hooks (config-driven, shell commands) or is a separate mechanism for agent-specific hooks. If the latter, rename to `AgentWorkspaceHook` to avoid confusion with the config-driven workspace hooks.

### Low

**L1. `ContinuePlan` and `RestorePlan` use `Vec<RuntimeStep>` — same as `LaunchPlan`.**

All three plan types are structurally identical: `{ steps: Vec<RuntimeStep> }`. The only type-level distinction is the struct name. This means a `LaunchPlan` could be accidentally passed where a `ContinuePlan` is expected (e.g., if a helper function accepts the inner `Vec<RuntimeStep>`). The type safety that `ShutdownPlan` achieves (via `ShutdownStep`) does not extend to the other plan types.

This is acknowledged in the design doc (lines 319-320) with the rationale that the valid operations are the same. The distinction is primarily documentary. Acceptable for MVP but worth noting as a deliberate trade-off.

**L2. `attach_info()` returns `Option<String>` — could benefit from a structured type.**

The Runtime trait's `attach_info()` (ADR line 89, design doc line 153) returns an optional string (e.g., `"tmux attach -t foo"`). This is used for `ao status` display and human copy-paste. A structured type (e.g., `AttachInfo { command: String, description: String }`) would be more useful for the CLI and dashboard, but the string is fine for MVP.

**L3. The factory function `create_agent()` takes `&AgentConfig` but `create_runtime()` takes `&Config`.**

Design doc lines 50-68 show asymmetric signatures: the agent factory receives only agent-specific config, while the runtime factory receives the full config. This is reasonable (the runtime may need `sessionPrefix`, `port`, etc.) but should be documented as intentional. If the runtime only needs a subset, a `RuntimeConfig` struct would be cleaner.

**L4. No mention of session ID format or collision prevention.**

The Runtime trait methods take `session_id: &str`. The design doc (line 226) mentions session naming as `{sessionPrefix}-{issueId}`, validated to be tmux-safe. But there is no discussion of what happens if two sessions for the same issue are spawned (e.g., retry after the first was killed). The session ID format and uniqueness guarantees should be specified, even if the answer is "the lifecycle engine ensures uniqueness."

**L5. `ProcessRuntime` is listed as "fast-follow" but `SendBuffer` is noted as unsupported.**

Design doc line 202 says the `process` runtime cannot do `SendBuffer` (no tmux buffer). If `process` is meant to be a lightweight fallback, the agents that produce `SendBuffer` steps (via `ao send` with large messages) will fail. This is a known limitation but should be noted in the deferred table.

## Summary

ADR-0004 is a well-designed, well-motivated proposal. The declarative plan pattern is the central insight and it delivers on its promises: decoupled traits, uniform execution, testable plans, and extensible delivery modes. The design integrates cleanly with the three prior ADRs and covers the PRD's FR1 and FR3 requirements with appropriate MVP scoping.

The three High findings should be addressed before acceptance:
- **H1** (`detect_activity()` hidden I/O) undermines the purity claim and the gather-phase I/O model. Making the I/O explicit preserves the design's testability story.
- **H2** (`ShutdownStep` has no execution path) will force a breaking trait change post-MVP. A forward-compatible stub now avoids this.
- **H3** (plan step failure semantics) is a gap that will surface immediately in production. `WaitForReady` timeouts are a common failure mode.

The Medium findings are design clarifications that would strengthen the ADR. M3 (prompt delivery mode) and M5 (WorkspaceHook definition) are the most impactful — they address gaps where the design references concepts without defining them.

**Recommendation:** Address H1, H2, and H3, then accept.
