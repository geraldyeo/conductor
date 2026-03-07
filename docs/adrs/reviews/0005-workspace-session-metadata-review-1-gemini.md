# ADR-0005 Review — Round 1 (Gemini)

**ADR version reviewed:** 0005-workspace-session-metadata.md as of 2026-03-07 (status: Proposed)
**Design doc reviewed:** docs/plans/2026-03-07-workspace-session-metadata-design.md

## Strengths

1. **Clean separation of Workspace trait and SessionStore module.** The decision to make these fully independent (Option 1) is well-justified. The Workspace trait manages filesystem isolation; the SessionStore manages persistence. Neither imports the other. The lifecycle engine coordinates between them, consistent with ADR-0004's engine-as-coordinator pattern. This avoids the coupling problem Option 2 would create (forcing `clone` to duplicate metadata logic) and the god-object problem of Option 3.

2. **DataPaths is appropriately scoped.** The helper is pure path arithmetic — it computes paths but owns no state and performs no I/O (except `ensure_dirs()`). This is a refreshing alternative to the god-object pattern. Every caller (Workspace, SessionStore, lifecycle engine) gets the paths it needs without depending on a shared stateful abstraction. The `.origin` collision detection is a thoughtful safety mechanism.

3. **JSONL journal is the right choice for action logs.** Separating the append-only journal from the atomic-replace metadata file means each file gets the access pattern it deserves. The `dedupe_key` field in `JournalEntry` directly supports FR4 idempotency checks. This is cleaner than embedding indexed journal entries in the KEY=VALUE file (Option 5), which would have made the metadata file grow unbounded and complicated bash parsing.

4. **Atomic write and race-free creation are well-specified.** The `atomic_write()` pseudocode (fsync + rename) and `create_dir()` race-free reservation are correct POSIX primitives. The design doc shows both the implementation and the rationale (temp file prevents partial reads on crash; `create_dir` atomicity prevents duplicate session creation). These are production-ready patterns.

5. **Hook orchestration is centralized in the lifecycle engine.** Hooks are not part of the Workspace trait — the engine runs them. This means the `clone` workspace plugin (post-MVP) inherits hook orchestration for free. The ordering (config hooks before agent hooks) is consistent with ADR-0004's decision that agent hooks run after config hooks.

6. **Symlink escape prevention is defense in depth.** The `canonicalize()` + `starts_with()` check catches path traversal attacks (`../../etc/passwd`) that session ID validation alone would not prevent. The validation is in the workspace implementation (not the trait), which is the right level — different workspace plugins may have different boundary semantics.

7. **The unwind sequence for session creation failures is explicitly specified.** Steps 2-6 of the creation sequence have a documented rollback: destroy workspace (if created), archive metadata with `TERMINATION_REASON=spawn_failed`. This is a gap that ADR-0004 left open (H3 in my prior review addressed plan failure semantics but not the broader spawn sequence), and this ADR fills it cleanly.

## Findings

### Critical

None.

### High

**H1. `SessionMetadata.status` is a `String`, creating a type-safety gap at the most critical boundary.**

The `SessionMetadata` struct (ADR lines 190-205, design doc lines 201-216) uses `pub status: String` rather than a typed enum. The ADR's Consequences section (line 341) acknowledges this: "the store would need to import lifecycle status types" creating a circular dependency. However, the lifecycle engine writes status values into this string on every transition (ADR line 304), and reads them back on every gather (ADR line 301). This is the hottest path in the entire system.

A typo in a status string (`"workng"` instead of `"working"`) would silently corrupt session state and go undetected until a guard function fails to match. With 16 statuses and 30 transitions, this is a real risk during development.

The circular dependency concern is solvable: extract the `SessionStatus` enum into a shared types module (e.g., `packages/core/src/types/status.rs`) that both the lifecycle engine and SessionStore import. Neither module depends on the other — both depend on the shared type. This is the standard Rust pattern for breaking circular dependencies.

**Evidence:** ADR line 341: "the store would need to import lifecycle status types"; design doc line 203: `pub status: String`.

**Recommendation:** Define `SessionStatus` as a shared enum in a common types module. `SessionMetadata.status` becomes `SessionStatus`. The KEY=VALUE serialization maps enum variants to/from strings (via `serde` or a manual `Display`/`FromStr` impl). This gives compile-time safety on the critical path without creating circular dependencies.

**H2. The unwind sequence has a gap: step 7 (`beforeRun` hook) failure is not covered.**

The ADR (lines 282-292) specifies that if steps 2-6 fail, the engine unwinds. But the creation sequence has 9 steps (lines 282-292), and step 7 is `Run config hook: beforeRun`. Step 8 is `Set env`, step 9 is `Execute LaunchPlan`. The unwind clause says "If steps 2-6 fail" but step 7 (`beforeRun`) is after the unwind boundary and before plan execution.

If `beforeRun` fails:
- The workspace has been created (step 3) and hooks have run (steps 5-6).
- The LaunchPlan has not been executed (step 9).
- The session is in a limbo state: workspace exists, metadata says it was created, but no agent is running.

The lifecycle engine would need to either (a) destroy the workspace and archive with `spawn_failed`, or (b) transition to `errored` and let the next poll cycle handle it. Neither is specified.

**Evidence:** ADR lines 282-292: "If steps 2-6 fail, unwind"; design doc lines 336-348: same boundary.

**Recommendation:** Extend the unwind boundary to cover steps 2-8 (everything before `Execute LaunchPlan`). Any failure in the pre-launch sequence should destroy the workspace and archive metadata with `TERMINATION_REASON=spawn_failed`. Step 9 (plan execution) failures are covered by ADR-0004's plan failure semantics (transition to `errored`, destroy runtime).

**H3. `append_journal()` is not atomic — concurrent appends or crash mid-write can corrupt the JSONL file.**

The design doc (lines 259-260) specifies `append_journal()` as opening the file in append mode and writing a single line. However:

1. **Partial writes on crash:** If the process crashes mid-write, the JSONL file will have a truncated last line. `read_journal()` (which parses line-by-line) will fail on the corrupt line and either return an error for the entire journal or silently skip the entry. Neither behavior is specified.

2. **Concurrent appends:** If multiple async tasks append to the same journal file simultaneously (e.g., the reaction engine and the lifecycle engine both executing entry actions for the same session), the writes could interleave, producing corrupt JSON on a line. `O_APPEND` guarantees atomic writes only up to `PIPE_BUF` (typically 4096 bytes on Linux/macOS). Journal entries could exceed this for large `target` or `error_code` values.

The metadata file avoids both problems via atomic write (temp + rename). The journal file has no such protection.

**Evidence:** Design doc lines 259-260: "Opens file in append mode"; ADR line 234: `pub fn append_journal(...)`.

**Recommendation:** (a) For crash safety: `read_journal()` should skip malformed trailing lines (with a warning log) rather than failing the entire read. This is a standard JSONL resilience pattern. (b) For concurrency safety: either serialize journal appends through a mutex (acceptable at MVP scale), or use the same atomic-write pattern as metadata (read all entries, append new one, write temp + rename). Option (b) is heavier but guarantees correctness regardless of entry size.

### Medium

**M1. `validate_symlink()` has a TOCTOU race between validation and symlink creation.**

The design doc (lines 113-128) validates the symlink target via `canonicalize()` before creating the symlink. Between the `canonicalize()` call and the `std::os::unix::fs::symlink()` call, the filesystem could change (e.g., an attacker replaces a directory with a symlink to `/etc`). This is a classic time-of-check-time-of-use (TOCTOU) race.

In practice, the attack surface is limited: the attacker would need write access to the repo directory during the brief window between validation and creation, and the orchestrator runs locally (not as a privileged service). However, the design doc explicitly calls this "defense in depth" (line 131), so the gap is worth noting.

**Recommendation:** Document the TOCTOU limitation. For defense in depth, consider also validating the symlink target after creation (verify the created symlink resolves within the repo boundary). This closes the race at the cost of one additional `canonicalize()` call.

**M2. `WorkspaceCreateContext` does not include the `DataPaths` reference, yet `WorktreeWorkspace` needs it.**

The factory function (ADR line 147, design doc line 138) takes `paths: &DataPaths` and stores it in the workspace instance. But `WorkspaceCreateContext` (ADR lines 104-111) already includes `worktree_path: PathBuf`, which is computed from `DataPaths::worktree_path()`. This means the lifecycle engine must call `DataPaths::worktree_path(session_id)` and populate the context before calling `workspace.create()`.

However, `destroy()` and `exists()` take only `session_id: &str` (ADR lines 123-124). These methods need to compute the worktree path from the session ID — which requires `DataPaths`. The workspace instance stores `DataPaths` from the factory (via `WorktreeWorkspace::new(config, paths)`), so it can compute the path internally. But `list()` (ADR line 125) returns `Vec<WorkspaceInfo>` with absolute paths — where does it get the base path?

The design works because the workspace stores `DataPaths` internally, but this means the `Workspace` trait has a hidden dependency on path computation that is not visible in the trait signature. A `clone` workspace plugin would need its own path strategy, and the trait does not express this.

**Recommendation:** Document in the trait's doc-comments that implementations are expected to store their own path configuration (received via the factory function). Alternatively, consider passing `&DataPaths` to `destroy()` and `exists()` for consistency with `create()` — but this may over-constrain alternative implementations.

**M3. KEY=VALUE format cannot represent multi-line values or values containing newlines.**

The metadata format (ADR lines 162-178, design doc lines 168-185) uses `KEY=VALUE` with "split on first `=`" parsing. If any value contains a newline (e.g., a `TERMINATION_REASON` with a multi-line error message), the parser will treat subsequent lines as separate entries, corrupting the metadata.

Current fields are unlikely to contain newlines at MVP, but `TERMINATION_REASON` is a natural candidate for multi-line content (stack traces, error messages). The format has no escaping mechanism.

**Evidence:** Design doc line 187: "split each line on the first `=`".

**Recommendation:** Either (a) specify that values are single-line and the engine must truncate/sanitize before writing (simplest), or (b) adopt a simple escaping scheme (e.g., `\n` literal for newlines, `\\` for backslash). Option (a) is sufficient for MVP if `TERMINATION_REASON` is constrained to a short enum-like string (e.g., `"spawn_failed"`, `"budget_exceeded"`, `"manual_kill"`).

**M4. No `fsync` on the parent directory after `rename()` in `atomic_write()`.**

The `atomic_write()` function (design doc lines 279-286) calls `file.sync_all()` (fsync on the data file) and then `std::fs::rename()`. On POSIX, the rename is atomic with respect to other processes, but the directory entry update is not guaranteed to be durable until the parent directory is also fsynced. A power failure after `rename()` but before the directory entry is flushed could lose the rename.

This is a well-known subtlety in crash-safe file I/O. In practice, modern filesystems (ext4 with `journal_data_ordered`, APFS) make this unlikely, and the system runs on developer laptops (not data centers with unreliable power). But since the design explicitly claims "crash safety" (ADR line 242), the gap should be acknowledged.

**Recommendation:** Document that full crash safety (surviving power loss) requires an additional `fsync` on the parent directory. Defer implementation to post-MVP — the current approach is sufficient for process crashes (SIGKILL, panic) which are the realistic failure mode on developer machines.

**M5. `Workspace::destroy()` calls `git branch -D` which force-deletes the branch without checking merge status.**

The design doc (line 105) specifies `destroy()` runs `git branch -D {branch}`. The `-D` flag force-deletes the branch regardless of merge status. If the session was killed before the branch was merged, any committed but unmerged work is lost (it becomes dangling objects, eventually garbage-collected by git).

For a `killed` session, this may be intentional. But for `errored` sessions (where the work might be recoverable), force-deleting the branch destroys the only named reference to the work.

**Evidence:** Design doc line 105: `git -C {repo_path} branch -D {branch}`.

**Recommendation:** Use `git branch -d` (lowercase) by default, which refuses to delete unmerged branches. Fall back to `git branch -D` only when the session's terminal state indicates the work is definitively abandoned (e.g., `killed`, `cleanup`). For `errored` sessions, log a warning and skip branch deletion, or archive the branch name in the metadata for manual recovery.

### Low

**L1. `DataPaths` hash uses only 12 characters of SHA-256 — collision probability should be quantified.**

The ADR (line 67) and design doc (line 18) specify `sha256(canonical_config_path)[..12]` as the directory name prefix. 12 hex characters = 48 bits of entropy. By the birthday paradox, collision probability reaches 1% at approximately `sqrt(2 * 2^48) ~ 23.7 million` distinct config paths. This is astronomically unlikely for a local developer tool.

The `.origin` file provides a runtime collision check, so even in the theoretical collision case, the system errors rather than silently sharing state. The design is sound, but the 12-character choice should be documented with the collision math to prevent future "why not use the full hash?" questions.

**Recommendation:** Add a one-line comment in the `DataPaths` implementation noting the collision probability and that `.origin` provides a safety net.

**L2. `SessionStore::exists()` returns `bool` while all other methods return `Result<T, StoreError>`.**

The `exists()` method (ADR line 238, design doc line 272) returns `bool`, swallowing any I/O errors that occur when checking the filesystem. If the `sessions/` directory is inaccessible (permissions, NFS mount failure), `exists()` returns `false` rather than propagating the error. This could cause the engine to create a duplicate session when the original is merely inaccessible.

**Recommendation:** Change `exists()` to return `Result<bool, StoreError>` for consistency with the other methods.

**L3. `archive()` uses `rename()` which fails across filesystem boundaries.**

The design doc (line 327) specifies `archive()` as a rename from `sessions/{id}/` to `archive/{id}-{timestamp}/`. Since both directories are under the same `{root}`, they are on the same filesystem, so `rename()` will succeed. However, if a user symlinks `sessions/` or `archive/` to a different mount point, the rename will fail with `EXDEV`.

This is an edge case — the default layout keeps everything under one directory. But the error would be confusing (a cross-device link error during session cleanup).

**Recommendation:** Document that `sessions/` and `archive/` must reside on the same filesystem. Consider catching `EXDEV` and falling back to copy + delete if this constraint is relaxed post-MVP.

**L4. No hook timeout default is specified.**

The `run_hook()` function (ADR line 262, design doc line 371) takes a `timeout: Duration` parameter, but neither document specifies the default timeout value. The lifecycle engine must pass this value, presumably from config, but ADR-0003's `Hooks` struct does not include a `timeout` field.

**Evidence:** ADR line 262: `pub async fn run_hook(command: &str, ctx: &HookContext, timeout: Duration)`; ADR-0003 does not define a hook timeout field.

**Recommendation:** Specify a default hook timeout (e.g., 60 seconds) in this ADR, and note that a per-hook timeout configuration field should be added to `Hooks` in ADR-0003 or deferred to post-MVP with the default hardcoded.

**L5. `WorkspaceInfo` does not include the `base_branch` that was used to create the worktree.**

`WorkspaceCreateContext` includes `base_branch` (ADR line 108), but `WorkspaceInfo` (ADR lines 116-118) only returns `branch` (the new branch). The lifecycle engine may need to know the base branch later (e.g., for PR creation, diff computation). This information is not persisted in `SessionMetadata` either.

**Recommendation:** Either add `base_branch` to `WorkspaceInfo` and `SessionMetadata`, or document that the lifecycle engine should retain `base_branch` from the creation context for later use.

## Summary

ADR-0005 is a solid, well-structured design that fills the critical data-layer gap beneath the lifecycle engine. The five-component decomposition (DataPaths, Workspace trait, SessionStore, hooks, lifecycle integration) is clean, and each component is independently testable. The design integrates well with ADRs 0001, 0003, and 0004 — the references are accurate and the contracts are consistent.

The three High findings should be addressed before acceptance:
- **H1** (status as String) introduces a type-safety gap on the system's hottest path. A shared enum in a common types module solves the circular dependency concern cleanly.
- **H2** (unwind boundary excludes step 7) leaves a gap in the spawn failure sequence that could result in orphaned workspaces.
- **H3** (journal append atomicity) could cause data corruption under concurrent writes or crash conditions — the very scenarios the design claims to handle.

The Medium findings are design refinements: M3 (KEY=VALUE newline handling) and M5 (`git branch -D` force-deletion) are the most impactful, as they could cause data loss in edge cases.

**Recommendation:** Address H1, H2, and H3, then accept.
