# ADR-0005 Review --- Round 2 (Codex)

**ADR version reviewed:** 0005-workspace-session-metadata.md as of 2026-03-07 (status: Proposed, post-Round-1 revisions)
**Design doc reviewed:** docs/plans/2026-03-07-workspace-session-metadata-design.md

## Round 1 Finding Resolution

### Codex Round 1 Findings

**H1. `SessionMetadata.status` is a bare `String` -- type-safety gap.**
**Resolved.** `SessionStatus` and `TerminationReason` are now shared enums in `packages/core/src/types/status.rs`, imported by both SessionStore and the lifecycle engine. `SessionMetadata.status` is typed as `SessionStatus`, and `termination_reason` is typed as `TerminationReason`. Both use `strum`'s `Display`/`FromStr` for KEY=VALUE serialization. This is exactly the shared-types-module pattern recommended. The `TerminationReason` enum also replaces the free-form string, closing the related gap.

**H2. KEY=VALUE format does not handle newlines or special characters.**
**Resolved.** The ADR now specifies newline escaping (`\n` to literal `\n`, `\\` for backslash) with the deserializer reversing the transformation. The "bash-compatible" claim has been replaced with an explicit warning against `source`-ing the file with untrusted values. The design doc's testing strategy (Section 6) now includes property tests with newlines, backslashes, `=`, Unicode, and shell-injection characters (`$()`, backticks). Comprehensive.

**H3. Unwind sequence gap -- workspace located via metadata rather than DataPaths.**
**Resolved.** The ADR now explicitly states: "Workspace destruction uses `DataPaths::worktree_path(session_id)` directly -- not the `WORKSPACE_PATH` field from metadata, since step 5 (which records it) may not have completed." This makes the unwind independent of metadata state, as recommended.

**M1. `Workspace::destroy()` force-deletes branch with `git branch -D`.**
**Resolved.** The ADR now specifies `git branch -d` (lowercase) by default, falling back to `git branch -D` only when the session's termination reason indicates work is definitively abandoned (e.g., `manual_kill`). If `-d` fails on unmerged work, the branch is preserved and a warning is logged. The `destroy()` method discovers the branch name via `git worktree list --porcelain` matching the worktree path. This also addresses M5 (destroy needs branch name).

**M2. `append_journal()` lacks `fsync` guarantees.**
**Resolved.** The ADR now specifies that `append_journal()` opens the file in append mode, writes one JSONL line, and calls `fsync` before returning. The journal is explicitly described as the idempotency mechanism for FR4 reactions, justifying the durability guarantee.

**M3. `DataPaths` hash collision -- no protection against `.origin` deletion.**
**Resolved.** The ADR now specifies that `ensure_dirs()` refuses to proceed if `sessions/` is non-empty but `.origin` is missing, preventing silent state adoption after manual `.origin` deletion. The collision probability is also quantified: ~23.7 million distinct paths before 1% collision probability.

**M4. No hook timeout default specified.**
**Resolved.** `DEFAULT_HOOK_TIMEOUT` is now defined as 60 seconds. Per-hook timeout configuration is explicitly deferred to post-MVP as an ADR-0003 extension.

**M5. `Workspace::destroy()` takes only `session_id` but needs branch name.**
**Resolved.** The worktree implementation discovers the branch name via `git worktree list --porcelain` matching the worktree path. The trait doc-comments note that implementations store their own path configuration received via the factory function. This is option (c) from the original recommendation.

**M6. No `Workspace::info()` method for single-session lookup.**
**Resolved.** `info(&self, session_id: &str) -> Result<Option<WorkspaceInfo>, WorkspaceError>` has been added to the trait.

### Gemini Round 1 Findings

**H1. `SessionMetadata.status` is a `String` -- type-safety gap.**
**Resolved.** Same resolution as Codex H1 above. Shared enums in `types/status.rs`.

**H2. Unwind boundary excludes step 7 (`beforeRun` hook failure).**
**Resolved.** The unwind boundary now covers steps 2-8 (the ADR says "steps 2 through 8"), which includes `beforeRun` (step 8 in the renumbered sequence) and all pre-launch steps. Step 9 is env setup (cannot fail), step 10 is plan execution (covered by ADR-0004). The ADR also correctly renumbered the steps: `PostToolUse` hook installation is now step 7, `beforeRun` is step 8.

**H3. `append_journal()` crash safety and concurrency.**
**Partially resolved.** Crash safety is addressed: `read_journal()` now skips malformed trailing lines with a warning log, which is the standard JSONL resilience pattern. `fsync` after each append provides durability. However, the concurrency concern (interleaved writes from multiple async tasks exceeding `PIPE_BUF`) is not explicitly addressed. At MVP scale, the lifecycle engine is single-threaded per session (one poll loop), so concurrent appends to the same session's journal are unlikely. The reaction engine (FR4) is post-MVP. This is acceptable for now, but see New Finding M1 below.

**M1. TOCTOU race in symlink validation.**
**Resolved.** The TOCTOU window is now explicitly documented and accepted: "the attacker would need write access to the repo directory during the brief window, and the orchestrator runs locally (not as a privileged service)." This is a reasonable risk acceptance for a local developer tool.

**M2. Workspace trait has hidden dependency on path computation.**
**Resolved.** The ADR now documents that "implementations store their own path configuration (received via the factory function); the trait signature uses only `session_id` for lookup/destroy operations, and implementations derive paths internally." This makes the contract explicit.

**M3. KEY=VALUE cannot represent multi-line values.**
**Resolved.** Same resolution as Codex H2. Newline escaping with `\n` literal, plus `TerminationReason` is now a constrained enum (not free-form), eliminating the most likely source of multi-line values.

**M4. No `fsync` on parent directory after `rename()`.**
**Resolved.** The ADR now explicitly acknowledges this: "full power-loss durability would require an additional `fsync` on the parent directory; this is deferred to post-MVP since process crashes (SIGKILL, panic) are the realistic failure mode on developer machines." Clear risk acceptance with documented upgrade path.

**M5. `git branch -D` force-deletes unmerged branches.**
**Resolved.** Same resolution as Codex M1. Conditional deletion with `-d`/`-D` based on termination reason.

**L1. Hash collision probability should be quantified.**
**Resolved.** The ADR now includes: "48 bits of entropy, ~23.7 million distinct paths before 1% collision probability."

**L2. `SessionStore::exists()` returns `bool` instead of `Result`.**
**Resolved.** Both `SessionStore::exists()` and `Workspace::exists()` now return `Result<bool, _>`.

**L3. `archive()` fails across filesystem boundaries.**
**Resolved.** The design doc now notes: "sessions/ and archive/ must reside on the same filesystem (both are under {root}/); rename() would fail with EXDEV across mount points."

**L4. No hook timeout default.**
**Resolved.** Same as Codex M4. `DEFAULT_HOOK_TIMEOUT = 60 seconds`.

**L5. `WorkspaceInfo` does not include `base_branch`.**
**Resolved.** `WorkspaceInfo` now includes `base_branch`, and `SessionMetadata` includes `BASE_BRANCH` in the KEY=VALUE file.

### Codex Round 1 Low Findings

**L1. Archive timestamp timezone ambiguity.**
**Resolved.** The ADR now specifies `{YYYYMMDD}T{HHMMSS}Z` (uppercase Z, UTC) and "ISO 8601 basic format."

**L2. `SessionStore::exists()` returns `bool`.**
**Resolved.** Returns `Result<bool, StoreError>`.

**L3. `symlinks: Vec<String>` should be `Vec<PathBuf>`.**
**Resolved.** Now `Vec<PathBuf>`.

**L4. `SessionMetadata` lacks doc comment about custom serialization.**
**Resolved.** Doc comment added: "Uses custom KEY=VALUE serialization (not serde). Values are single-line with newline escaping."

**L5. `beforeRemove` failure semantics unspecified.**
**Resolved.** The ADR now explicitly states: "beforeRemove hook failures are logged but do not prevent workspace destruction or archival."

## New Findings

### Critical

None.

### High

None.

### Medium

**M1. Concurrent journal appends are not addressed for post-MVP reaction engine.**

Gemini R1 H3 raised the concern that concurrent appends from multiple async tasks could interleave beyond `PIPE_BUF`. The current ADR addresses crash safety (fsync, malformed-line skipping) but does not address write interleaving. The reaction engine (FR4, post-MVP) will introduce concurrent journal writers for the same session. `O_APPEND` guarantees atomicity only up to `PIPE_BUF` (4096 bytes on Linux/macOS), and journal entries with large `error_code` or `target` fields could theoretically exceed this.

At MVP scale this is a non-issue (single poll loop per session). However, since the journal is explicitly described as the idempotency mechanism for FR4 reactions, the ADR should acknowledge this limitation and note that a serialization mechanism (e.g., per-session mutex or channel) will be needed when the reaction engine is introduced.

**Recommendation:** Add a one-line note in the Consequences or Deferred section: "Concurrent journal appends from multiple writers (e.g., reaction engine) will require serialization; deferred to FR4 implementation."

### Low

**L1. `destroy()` branch deletion strategy relies on `git worktree list --porcelain` parsing.**

The ADR specifies that `destroy()` discovers the branch name by running `git worktree list --porcelain` and matching the worktree path. The `--porcelain` format is stable across git versions, but the ADR does not specify which fields are matched or how the output is parsed. If the worktree has already been partially removed (e.g., the directory was deleted manually but `git worktree prune` was not run), the worktree may still appear in the list with a different state.

This is documented in the Consequences section ("depends on `git worktree` CLI behavior") and mitigated by integration testing. The risk is low -- `--porcelain` is explicitly designed for machine parsing -- but the specific parsing contract (which lines/fields are matched) should be specified during implementation.

**Recommendation:** No ADR change needed. Document the expected `--porcelain` output format in the implementation (code-level comments).

**L2. `TerminationReason::None` as a sentinel value.**

The `TerminationReason` enum includes a `None` variant to represent "not terminated." In Rust, this shadows the `Option::None` variant, which could cause confusion in pattern matching (e.g., `Some(TerminationReason::None)` is semantically awkward). An alternative would be to use `Option<TerminationReason>` with no `None` variant in the enum, where `Option::None` means "not terminated."

This is a stylistic preference, not a correctness issue. The current design works and serializes cleanly to `TERMINATION_REASON=none` in the KEY=VALUE file.

**Recommendation:** Consider renaming to `TerminationReason::NotTerminated` or using `Option<TerminationReason>` during implementation. No ADR change required.

## Verdict

**Accept.** All High and Medium findings from Round 1 (both Codex and Gemini) have been adequately addressed. The revisions are thorough: shared status enums, newline escaping, expanded unwind boundary, journal fsync, conditional branch deletion, and explicit risk acceptances for TOCTOU and parent-directory fsync. The one new Medium finding (M1, concurrent journal appends) is a post-MVP concern that can be addressed with a brief note in the Deferred section. The Low findings are implementation-level details, not design gaps. The ADR is ready for acceptance.
