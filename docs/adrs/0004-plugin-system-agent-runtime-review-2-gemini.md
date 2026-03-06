# ADR-0004 Review — Round 2 (Gemini)

**ADR version reviewed:** 0004-plugin-system-agent-runtime.md as of 2026-03-07 (status: Proposed)
**Design doc reviewed:** docs/plans/2026-03-06-plugin-system-agent-runtime-design.md
**Round 1 review:** docs/adrs/0004-plugin-system-agent-runtime-review-1-gemini.md

## Round 1 Fix Verification

### H1. `detect_activity()` hidden I/O — RESOLVED

The revision introduces `GatherContext` with `terminal_output`, `auxiliary_log`, and `auxiliary_log_path` fields. The gatherer reads all I/O (including agent-specific JSONL logs discovered via `agent.auxiliary_log_path()`), and Agent methods (`detect_activity`, `parse_session_info`) are now pure functions over this context. The design doc (Section 3, lines 296-311) and ADR (lines 122-139) are consistent on this. The testing examples (design doc Section 7) construct `GatherContext` literals and assert without mocks, confirming the purity claim holds.

Well done. This is a clean fix that preserves the gather-phase I/O model from ADR-0001.

### H2. `ShutdownStep` no execution path — RESOLVED

The ADR (lines 161-165) and design doc (lines 346-350) now explicitly document that the lifecycle engine maps `ShutdownStep` variants to direct Runtime method calls rather than routing through `execute_step()`. The mapping is concrete: `SendMessage` wraps into `RuntimeStep::SendMessage`, `WaitForExit` polls `is_alive()`, `ForceKill` and `DestroyRuntime` both call `destroy()`. The rationale for the asymmetry (shutdown involves lifecycle-level escalation decisions) is stated in both documents.

This matches my round 1 recommendation option (b). The gap is closed.

### H3. Plan step failure semantics — RESOLVED

The ADR adds a dedicated "Plan Execution Failure Semantics" section (Section 6, lines 167-179) specifying: abort on first failure, cleanup via `runtime.destroy()` if `Create` succeeded, transition to `errored` with step index and error in metadata, structured logging per step. The design doc (Section 4, lines 598-610) mirrors this with updated pseudocode showing the error handling path. Common failure modes are enumerated.

This is thorough and addresses the gap completely.

### M1. `supports_step()` missing — RESOLVED

Added to the Runtime trait as `fn supported_steps(&self) -> &'static [&'static str]` with a default returning all step types (ADR line 90-92, design doc lines 158-161). Plan validation via `validate_plan()` is now concrete (design doc Section 6, lines 629-649) with `RuntimeStep::type_name()` matching against `supported_steps()`. The tmux implementation excludes `"send_protocol"`.

Clean addition. The string-based matching (`&'static [&'static str]`) is slightly loose compared to an enum-based approach, but pragmatic for MVP.

### M2. `SessionInfo` scope — RESOLVED

Renamed to `AgentSessionInfo` in both ADR (lines 141-149) and design doc (lines 414-424). Doc comments clarify it is agent-extracted metadata, distinct from the full session record. The ADR's PRD mapping table (line 197) uses the new name consistently.

### M3. `prompt_delivery` missing from `AgentConfig` — RESOLVED

Added as a typed `PromptDelivery` enum (`Inline`, `PostLaunch`, `Protocol`) with `Option<PromptDelivery>` on `AgentConfig` (ADR lines 229-244, design doc lines 446-458). The Agent reads `ctx.agent_config.prompt_delivery` in `launch_plan()` to decide the step sequence. This is the correct place for it — a cross-agent concern, not relegated to the `extra` HashMap.

### M4. `getMetrics()` / token tracking — RESOLVED

The ADR's PRD mapping table (line 211) explicitly addresses `getMetrics()`: deferred because the primary token data source is agent JSONL logs, not the runtime. `parse_session_info()` is documented as called every poll tick (ADR line 152, design doc line 427) for fresh token counts, enabling budget evaluation. The pipeline is clear.

### M5. `WorkspaceHook` undefined — RESOLVED

Defined as `AgentWorkspaceHook` enum with `PostToolUse` and `AfterCreate` variants (ADR lines 219-225, design doc lines 434-439). The relationship between agent hooks (programmatic, from the Agent trait) and config hooks (shell commands in YAML, from FR2) is explicitly clarified. Agent hooks run after config hooks. The naming distinction (`AgentWorkspaceHook` vs. config-driven workspace hooks) prevents confusion.

## New Findings

### Critical

None.

### High

None.

### Medium

**M1. `auxiliary_log_path()` returns a directory glob, not a file path.**

The ADR (line 138) states Claude Code returns `Some(~/.claude/projects/.../logs/*.jsonl)` — this is a glob pattern, not a file path. The `GatherContext.auxiliary_log_path` field is typed as `Option<PathBuf>`, which cannot represent a glob. The design doc (line 408) shows the method returning `Some(self.jsonl_log_dir.clone())`, which looks like a directory path.

Several questions are unresolved:
- Does the gatherer read a single file, the latest file in a directory, or all files matching a glob?
- For a long-running session, the JSONL log may grow large. The ADR's Consequences section (line 291) mentions "reading only the tail," but the gathering mechanism (glob vs. single file vs. directory scan) is not specified.

**Recommendation:** Clarify the contract. If `auxiliary_log_path()` returns a directory, document that the gatherer reads the most recent file in that directory (or the tail of it). If it returns a file path, fix the ADR's glob example. This affects performance (reading entire log files every 30s tick) and correctness (which file to read when multiple exist).

**M2. `ForceKill` and `DestroyRuntime` both map to `runtime.destroy()` — redundant.**

The `ShutdownStep` enum has both `ForceKill` and `DestroyRuntime`, and both are mapped to `runtime.destroy()` (ADR line 165, design doc line 350). If they have the same execution semantics, why are they distinct variants?

Possible justifications: (a) semantic distinction for logging/journaling (one means "kill the process," the other means "tear down the runtime resource"), (b) future divergence (e.g., `ForceKill` sends SIGKILL while `DestroyRuntime` removes the tmux session). Neither is documented.

**Recommendation:** Add a one-line comment distinguishing the intent of each variant, or collapse them into a single `Destroy` variant if they are truly identical. This is a minor clarity issue but will confuse implementers.

### Low

**L1. `supported_steps()` uses string matching — fragile coupling.**

`RuntimeStep::type_name()` returns strings like `"create"`, `"send_message"` that must match the strings in `supported_steps()`. This coupling is maintained by convention, not by the type system. A typo in either side (e.g., `"send_msg"` vs. `"send_message"`) would silently pass validation.

At MVP with one runtime this is low risk. Post-MVP, consider an enum-based capability declaration (e.g., `RuntimeStepKind` discriminant) to get compile-time safety. Acceptable for now.

**L2. `GatherContext` lacks session metadata for contextual detection.**

`GatherContext` provides terminal output and auxiliary log content, but not the session's current status or elapsed time in that status. Some agents might benefit from knowing how long the session has been running or what state the engine thinks it is in, to disambiguate terminal output patterns. This is speculative and not needed at MVP, but worth noting if detection logic proves insufficient.

## Consistency Check

The ADR and design doc are consistent after revisions. Specifically:

- `GatherContext` struct fields match between ADR (lines 125-129) and design doc (lines 303-307).
- `AgentSessionInfo` name and fields are consistent across both documents.
- `ShutdownStep` execution path description is identical in both.
- Plan failure semantics appear in both ADR Section 6 and design doc Section 4.
- `PromptDelivery` enum definition matches between ADR (lines 237-240) and design doc (lines 452-455).
- `AgentWorkspaceHook` enum variants match between ADR (lines 219-225) and design doc (lines 434-439).
- The PRD mapping tables in the ADR are complete and accurate.
- `supported_steps()` signature and defaults are identical in both documents.

No inconsistencies found.

## Verdict

All three High findings and all five Medium findings from round 1 have been adequately addressed. The revisions are well-integrated — they do not feel bolted on, and both documents tell a coherent story.

The two new Medium findings (M1: auxiliary log path ambiguity, M2: ForceKill/DestroyRuntime redundancy) are clarification issues, not structural problems. They can be resolved with minor edits or deferred to implementation.

**Recommendation: Accept.** The ADR is ready to move from Proposed to Accepted. Address M1 and M2 during implementation or as minor follow-up edits.
