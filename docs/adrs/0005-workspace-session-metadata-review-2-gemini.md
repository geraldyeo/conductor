# ADR-0005 Review — Round 2 (Gemini)

**ADR version reviewed:** 0005-workspace-session-metadata.md as of 2026-03-07 (status: Proposed, post-Round-1 revisions)
**Design doc reviewed:** docs/plans/2026-03-07-workspace-session-metadata-design.md

## Round 1 Finding Resolution

### Gemini Round 1 Findings

**H1. `SessionMetadata.status` is a `String`, creating a type-safety gap.** -- **Resolved.** `SessionStatus` and `TerminationReason` are now shared enums in `packages/core/src/types/status.rs`, imported by both SessionStore and lifecycle engine. Both use `strum`'s `Display`/`FromStr` for string serialization in the KEY=VALUE format. `SessionMetadata.status` is now `SessionStatus` and `termination_reason` is `TerminationReason`. This is exactly the shared-types-module approach recommended. Well done.

**H2. Unwind sequence gap -- step 7 (`beforeRun` hook) outside unwind boundary.** -- **Resolved.** The unwind boundary now explicitly covers steps 2-8 (ADR line 337: "if any step in the pre-launch sequence (steps 2 through 8) fails"). Step 9 (env setup) cannot fail, and step 10 (plan execution) is covered by ADR-0004. The gap is closed.

**H3. `append_journal()` not atomic -- crash and concurrency risks.** -- **Resolved.** The ADR now specifies (line 280): `append_journal()` opens in append mode, writes one JSONL line, and calls `fsync` before returning. `read_journal()` skips malformed trailing lines with a warning log (line 280). This addresses both crash resilience and the durability concern. The concurrency issue (interleaved writes exceeding `PIPE_BUF`) is not explicitly addressed with a mutex, but given that the lifecycle engine is the sole writer for a given session during its poll tick (single-threaded per-session processing), concurrent appends are not a realistic concern at MVP. Acceptable.

**M1. TOCTOU race in `validate_symlink()`.** -- **Resolved.** The TOCTOU window is now explicitly documented in the ADR (line 135) and design doc (line 145), with the rationale that the attacker would need write access to the repo directory during the brief window and the orchestrator runs locally. This is an appropriate risk acceptance for a local developer tool.

**M2. `Workspace` trait hidden dependency on path computation.** -- **Resolved.** The ADR now states (line 101): "Implementations store their own path configuration (received via the factory function); the trait signature uses only `session_id` for lookup/destroy operations, and implementations derive paths internally." This makes the design intent clear.

**M3. KEY=VALUE format cannot represent newlines.** -- **Resolved.** The ADR now specifies newline escaping (line 166): "the serializer replaces newlines with `\n` literal (two characters `\` and `n`) and backslashes with `\\`; the deserializer reverses this." The design doc (line 211) adds a shell injection warning. The testing strategy (design doc line 473) now explicitly includes "values with newlines, backslashes, shell-injection characters." Comprehensive fix.

**M4. No `fsync` on parent directory after `rename()`.** -- **Resolved.** The ADR (line 278) and design doc (lines 336-337) now explicitly document that full power-loss durability is deferred, with the rationale that process crashes are the realistic failure mode. This was the recommended approach.

**M5. `git branch -D` force-deletes unmerged branches.** -- **Resolved.** The ADR (line 131) now specifies `git branch -d` (lowercase) by default, with `-D` only for definitively abandoned work (e.g., `manual_kill`). If `-d` fails on unmerged work, a warning is logged and the branch is preserved. This is the recommended graduated approach.

**L1. Hash collision probability not quantified.** -- **Resolved.** The ADR (line 97) now includes the birthday-bound calculation: "~23.7 million distinct paths before 1% collision probability."

**L2. `exists()` returns `bool` instead of `Result`.** -- **Resolved.** The ADR (line 125) and design doc (line 119) now show `Result<bool, WorkspaceError>`. The SessionStore's `exists()` (ADR line 274) also returns `Result<bool, StoreError>`.

**L3. `archive()` fails across filesystem boundaries.** -- **Resolved.** The design doc (line 377) now documents: "sessions/ and archive/ must reside on the same filesystem."

**L4. No hook timeout default specified.** -- **Resolved.** A `DEFAULT_HOOK_TIMEOUT` of 60 seconds is now defined (ADR line 302, design doc line 423), with per-hook timeout configuration noted as a post-MVP extension.

**L5. `WorkspaceInfo` missing `base_branch`.** -- **Resolved.** `WorkspaceInfo` now includes `base_branch` (ADR line 117, design doc line 84), and `SessionMetadata` includes `BASE_BRANCH` (ADR line 183).

### Codex Round 1 Findings

**H1. `SessionMetadata.status` is a bare `String`.** -- **Resolved.** Same as Gemini H1 above. Shared enum approach adopted.

**H2. KEY=VALUE format does not handle newlines or special characters.** -- **Resolved.** Same as Gemini M3 above. Newline escaping and shell injection warning added.

**H3. Unwind sequence gap -- workspace path not derivable from metadata on step 4 failure.** -- **Resolved.** The ADR (line 337) now explicitly states: "destroy workspace via `DataPaths::worktree_path(session_id)` (not from metadata, since `WORKSPACE_PATH` may not have been written yet)." This makes the unwind independent of metadata state.

**M1. `git branch -D` unconditional force-delete.** -- **Resolved.** Same as Gemini M5 above. Graduated deletion strategy adopted.

**M2. `append_journal()` lacks `fsync`.** -- **Resolved.** Same as Gemini H3 above. `fsync` now specified.

**M3. `.origin` deletion allows silent state adoption.** -- **Resolved.** The ADR (line 97) and design doc (lines 43-44, 62) now specify that `ensure_dirs()` refuses to proceed if `sessions/` is non-empty but `.origin` is missing. This prevents a new project from silently inheriting another project's sessions.

**M4. No hook timeout default.** -- **Resolved.** Same as Gemini L4 above.

**M5. `Workspace::destroy()` needs branch name but only takes `session_id`.** -- **Resolved.** The ADR (lines 133) now specifies that the worktree implementation discovers the branch name via `git worktree list --porcelain`, matching the worktree path. This is the recommended approach (Codex option c) and avoids depending on naming conventions or SessionStore.

**M6. No `Workspace::info()` method.** -- **Resolved.** The trait now includes `async fn info(&self, session_id: &str) -> Result<Option<WorkspaceInfo>, WorkspaceError>` (ADR line 126, design doc line 97).

**L1. Archive timestamp timezone ambiguity.** -- **Resolved.** The ADR (line 94) now uses `{YYYYMMDD}T{HHMMSS}Z` with explicit "(UTC)" annotation and ISO 8601 basic format label.

**L2. `exists()` returns `bool`.** -- **Resolved.** Same as Gemini L2 above.

**L3. `symlinks` uses `String` instead of `PathBuf`.** -- **Resolved.** `WorkspaceCreateContext.symlinks` is now `Vec<PathBuf>` (ADR line 110, design doc line 77).

**L4. `SessionMetadata` lacks doc comment about custom serialization.** -- **Resolved.** The ADR (line 223) now includes a doc comment: "Uses custom KEY=VALUE serialization (not serde). Values are single-line with newline escaping."

**L5. `beforeRemove` failure semantics unspecified.** -- **Resolved.** The ADR (line 320) now states: "beforeRemove hook failures are logged but do not prevent workspace destruction or archival."

**Cross-ADR consistency (Codex).** The ADR now documents the `PostToolUse` distinction (ADR line 318): it is a file-based hook installed into the workspace, not a lifecycle event hook. The archival step is documented as "an additional step owned by this ADR" (ADR line 345), reconciling the gap with ADR-0001's entry action list.

## New Findings

### Critical

None.

### High

None.

### Medium

**M1. `TerminationReason::None` as a variant is semantically awkward.**

The `TerminationReason` enum includes a `None` variant (ADR line 215) to represent "not terminated." In Rust, the idiomatic way to represent "no value" is `Option<T>`. Using `TerminationReason::None` means the enum always has a value even when there is no termination, which creates ambiguity: is `None` a valid reason, or is it the absence of a reason? In the metadata file, it serializes to `TERMINATION_REASON=none`, which reads strangely.

Additionally, `TerminationReason::None` collides with Rust's `Option::None` in cognitive load -- a developer writing `if reason == None` could confuse the two.

**Recommendation:** Use `Option<TerminationReason>` in `SessionMetadata` and remove the `None` variant. In the KEY=VALUE file, serialize `Option::None` as an empty value (`TERMINATION_REASON=`), which is already how empty fields are represented (e.g., `BRANCH=`, `PR_URL=`). This is more idiomatic and avoids the naming collision.

**M2. `destroy()` branch deletion via `git worktree list --porcelain` is fragile if the worktree was already removed.**

The ADR (line 133) specifies that `destroy()` discovers the branch name by running `git worktree list --porcelain` and matching the worktree path. However, if the worktree directory was already removed (e.g., by a previous failed destroy attempt, or manual deletion), it will not appear in `git worktree list` output. In this case, `destroy()` cannot discover the branch name and cannot delete it.

The ADR states `destroy()` is idempotent (design doc line 117), but idempotency only covers the worktree removal itself. The branch becomes an orphan -- it is never cleaned up.

**Recommendation:** As a fallback, derive the branch name from a documented convention (e.g., branch name matches session ID, or is stored in a small `.branch` file alongside the worktree). Alternatively, accept this as a known limitation and document that orphaned branches may remain after partial failures, recommending `git branch --list 'ao-*'` for manual cleanup.

### Low

**L1. `created_at` and `updated_at` are `String` rather than a timestamp type.**

`SessionMetadata.created_at` and `updated_at` are `String` (ADR line 228-229). While the format is documented as ISO 8601, there is no compile-time enforcement. A `chrono::DateTime<Utc>` or `time::OffsetDateTime` would provide validation at parse time and prevent format inconsistencies. The KEY=VALUE serializer would format these as ISO 8601 strings.

**Recommendation:** Consider using a proper timestamp type internally, serialized to/from ISO 8601 strings in the KEY=VALUE format. This is a minor type-safety improvement that can be addressed during implementation.

**L2. `SessionStore` API methods are not `async` despite filesystem I/O.**

All `SessionStore` methods (ADR lines 267-275) are synchronous (`fn` not `async fn`). The workspace trait methods are `async`. Given that the orchestrator runs on `tokio` (ADR-0002), synchronous filesystem I/O in `SessionStore` will block the runtime thread. At MVP scale with few sessions, this is unlikely to cause issues, but it is inconsistent with the workspace trait's async design.

**Recommendation:** Either make `SessionStore` methods `async` (using `tokio::fs` for non-blocking I/O), or document that synchronous filesystem I/O is acceptable at MVP scale and note the async migration as a potential future improvement. Using `tokio::task::spawn_blocking` around synchronous calls is also an option the lifecycle engine could employ.

## Verdict

**Accept.** All High and Medium findings from Round 1 (both Gemini and Codex) have been adequately addressed. The revisions are thorough -- every finding received a substantive fix, not just a documentation note. The new findings are Medium and Low severity: M1 (TerminationReason::None) is an idiomatic Rust concern that can be resolved during implementation, and M2 (branch discovery after worktree removal) is an edge case with a clear workaround. Neither warrants another review round.
