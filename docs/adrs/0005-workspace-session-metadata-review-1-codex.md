# ADR-0005 Review — Round 1 (Codex)

**ADR version reviewed:** 0005-workspace-session-metadata.md as of 2026-03-07 (status: Proposed)
**Design doc reviewed:** docs/plans/2026-03-07-workspace-session-metadata-design.md

---

## Strengths

1. **Clean separation of Workspace and SessionStore.** Option 1 (fully independent modules) is the correct choice. The Workspace trait handles filesystem isolation; SessionStore handles persistence. Neither depends on the other. This makes each module independently testable and avoids the trap of Option 2 where the `clone` plugin would need to duplicate metadata logic.

2. **DataPaths as pure path arithmetic.** The `DataPaths` helper computes all paths without owning state or performing I/O (except `ensure_dirs()`). This avoids the god-object problem of Option 3 while centralizing the one thing that both modules need: consistent path computation. The `.origin` collision detection is a practical safeguard against the 12-char hash truncation.

3. **Separate file formats for separate access patterns.** Using KEY=VALUE for atomic-replace metadata and JSONL for append-only journals is a well-motivated split. The metadata file stays bash-scriptable (`source metadata`); the journal gets structured entries with `dedupe_key` for idempotency. Mixing these in one format would compromise both.

4. **Atomic write and race-free creation are correctly implemented.** The `fsync + rename` pattern for metadata writes and `create_dir` atomicity for session reservation are standard POSIX primitives used correctly. The design avoids inventing locking schemes.

5. **Hook orchestration is centralized in the lifecycle engine.** Keeping hook execution out of the Workspace trait follows the engine-as-coordinator pattern from ADR-0004. This prevents hook duplication across workspace plugins and gives the engine full control over ordering and failure handling.

6. **Unwind logic for session creation failures.** The ADR explicitly defines rollback behavior: if steps 3-7 fail, destroy the workspace (if created) and archive metadata with `TERMINATION_REASON=spawn_failed`. This addresses the gap identified as H1 in the ADR-0004 review, applied to the workspace/metadata layer.

7. **Session ID validation is defense in depth.** Rejecting empty, `/`, `\`, `\0`, `.`, `..`, and leading-dot IDs at the SessionStore boundary complements ADR-0004's tmux-safe format `[a-zA-Z0-9._-]`. Two layers of validation with different scopes is the right approach.

---

## Findings

### Critical

None.

### High

**H1. `SessionMetadata.status` is a bare `String`, creating a silent corruption vector.**

The ADR explicitly acknowledges this (Consequences, negative point 2): "Session metadata uses string types for `status` and `termination_reason` rather than enums... the lifecycle engine is responsible for writing valid values." The stated reason is to avoid a circular dependency between the store and lifecycle status types.

However, this is a high-severity design risk. The lifecycle engine writes status values on every transition (ADR-0005, Section 5: "Transition phase: `session_store.write()` with updated status"). A typo in a status string (e.g., `"workng"` instead of `"working"`) would silently persist and break recovery. On restart, the lifecycle engine loads sessions via `session_store.list()` and must parse the status string back into its internal enum. A malformed status would either crash the engine or silently discard the session.

The circular dependency concern is solvable without coupling the store to the lifecycle engine. The `SessionStatus` enum can live in a shared types module (e.g., `packages/core/src/types/status.rs`) imported by both the store and the lifecycle engine. Alternatively, `SessionMetadata.status` can use a `ValidatedStatus(String)` newtype that validates against a known set of values at construction, without importing the lifecycle engine's full type.

**Evidence:** ADR-0005 Section 3 (`SessionMetadata` struct, line 191: `pub status: String`); Consequences section (line 341).

**Recommendation:** Introduce a shared `SessionStatus` enum or a validated newtype wrapper. The store module validates on read; the lifecycle engine constructs valid values. This eliminates the class of bugs where a corrupted status string causes silent data loss on recovery.

**H2. Metadata KEY=VALUE format does not handle values containing newlines or special characters.**

The ADR specifies KEY=VALUE format parsed by "split each line on the first `=`" (design doc Section 3, line 187). The `SessionMetadata` struct includes `workspace_path` (PathBuf), `branch` (String), `pr_url` (String), and `termination_reason` (String). Consider these scenarios:

- A `TERMINATION_REASON` containing a newline (e.g., a multi-line error message) would corrupt the file by splitting across lines.
- A `WORKSPACE_PATH` containing `=` (legal on most filesystems) would parse correctly due to first-`=` splitting, but a `BRANCH` containing `\n` (unlikely but possible with Git) would not.
- The format is described as "bash-compatible," but bash `source` would execute arbitrary code if a value contained `$(command)` or backtick substitution.

The design doc's testing strategy (Section 6) mentions "KEY=VALUE serialization edge cases (values with `=`, empty values, Unicode)" but does not address newlines or shell injection.

**Evidence:** Design doc Section 3 (line 187: "split each line on the first `=`"); ADR Section 3 (line 162: "bash-compatible").

**Recommendation:** (a) Escape newlines in values during serialization (e.g., `\n` literal escape, decoded on read). (b) Drop the "bash-compatible" claim or add a warning that `source`-ing the file is unsafe with untrusted values. (c) Add property tests that generate values with newlines, `=`, quotes, backticks, and `$()` and verify round-trip correctness.

**H3. Unwind sequence has a gap between steps 2 and 3.**

The session creation sequence (ADR Section 5, lines 281-292) states: step 2 = `session_store.create()`, step 3 = `workspace.create()`. If step 3 fails, the unwind destroys the workspace (if created) and archives metadata. But the metadata written in step 2 has no `WORKSPACE_PATH` yet — that is written in step 4 (`session_store.write()` with `WORKSPACE_PATH`). The archived metadata for a step-3 failure will have an empty `WORKSPACE_PATH`, which is correct.

However, if step 4 itself fails (metadata write after successful workspace creation), the workspace exists but no metadata records its path. The unwind logic says "if steps 2-6 fail" but does not specify how to find the workspace to destroy it when the metadata write that would record its path is the step that failed. The engine must use `DataPaths::worktree_path(session_id)` directly rather than reading `WORKSPACE_PATH` from metadata.

**Evidence:** ADR Section 5, lines 281-292 ("If steps 2-6 fail, unwind: destroy workspace (if created)").

**Recommendation:** Clarify that the unwind logic uses `DataPaths::worktree_path(session_id)` to locate the workspace for destruction, not the `WORKSPACE_PATH` field from metadata. This makes the unwind independent of metadata state.

### Medium

**M1. `Workspace::destroy()` deletes the branch unconditionally with `git branch -D`.**

The worktree implementation's `destroy()` runs `git branch -D {branch}` (ADR Section 2, line 129; design doc Section 2, line 105). This force-deletes the branch regardless of whether it has been merged. If destruction is triggered by an error state (e.g., `errored`, `killed`) before the PR is merged, unmerged work is lost. The commits still exist in the reflog temporarily, but `git branch -D` is destructive.

The lifecycle engine's destruction sequence (Section 5, lines 296-299) calls `workspace.destroy()` from terminal-state entry actions including `cleanup` and `killed`. For `merged`, branch deletion is safe. For `errored` or `killed`, it may not be.

**Evidence:** ADR Section 2, line 129 (`git branch -D`); Section 5, line 297 ("Called from lifecycle engine entry actions for terminal states").

**Recommendation:** Use `git branch -d` (lowercase) for non-error terminal states, which refuses to delete unmerged branches. For error states, either (a) skip branch deletion entirely (the branch is a useful forensic artifact), or (b) document this as intentional since the archive directory preserves metadata. At minimum, add a log warning when force-deleting an unmerged branch.

**M2. `append_journal()` lacks `fsync` guarantees.**

The `atomic_write()` function for metadata correctly calls `file.sync_all()` before rename (design doc Section 3, line 283). However, `append_journal()` is described as "Opens file in append mode" (design doc Section 3, line 260) with no mention of `fsync`. If the orchestrator crashes after appending a journal entry but before the OS flushes the write buffer, the entry is lost.

For metadata, this is mitigated by atomic replace. For the journal, which is append-only and used for idempotency (`dedupe_key` checks), a lost entry means the same action could be retried. Whether this matters depends on the idempotency of the actions themselves (e.g., `merge` is idempotent at the GitHub API level, but `label` might add duplicate labels).

**Evidence:** Design doc Section 3, line 260 (`append_journal` — no fsync); line 283 (`atomic_write` — has fsync).

**Recommendation:** Call `file.sync_all()` after each `append_journal()` write, or document that journal durability is best-effort and actions must be independently idempotent. Given that journal entries are the idempotency mechanism for FR4 reactions, the former is safer.

**M3. `DataPaths` hash uses only 12 characters of SHA-256, with no birthday-bound analysis.**

The root directory uses `sha256(canonical_config_path)[..12]` — 12 hex characters = 48 bits of entropy. The birthday bound for a 50% collision probability is approximately `2^24` (~16 million) distinct config paths. In practice, a single user will have far fewer, so collisions are extremely unlikely. However:

- The `.origin` file detects collisions reactively (error on mismatch), not proactively. If an old `.origin` file is deleted manually, a new project could silently inherit state from a different project.
- The design doc (Section 1, line 57) says `ensure_dirs()` "reads `.origin` and compares it to the current config path." But `ensure_dirs()` also creates the `.origin` file if absent (line 39: "Write .origin if absent"). This means deleting `.origin` and running a different project with the same hash prefix would succeed — the new project writes a new `.origin` and claims the directory.

**Evidence:** Design doc Section 1, lines 24, 39, 57.

**Recommendation:** Add a check: if the `sessions/` directory is non-empty but `.origin` is missing, refuse to proceed rather than writing a new `.origin`. This prevents silent state adoption after `.origin` deletion.

**M4. `WorkspaceCreateContext` does not include hook timeout.**

The `HookContext` struct (ADR Section 4, line 255) includes session-level context, but the `run_hook()` function takes `timeout: Duration` as a parameter. There is no specification of where this timeout value comes from. ADR-0003's `Hooks` struct (design doc line 157) defines hooks as `Option<String>` (just the command), with no per-hook timeout configuration.

This means the timeout is either hardcoded or comes from a top-level config field that is not defined in any ADR. The PRD does not specify a hook timeout default.

**Evidence:** ADR-0005 Section 4, line 262 (`run_hook` takes `timeout: Duration`); ADR-0003 design doc line 157 (`Hooks` struct — no timeout field).

**Recommendation:** Define a default hook timeout (e.g., 60 seconds) in the ADR and note that per-hook timeouts are a post-MVP config extension. The default should be documented so implementers do not need to guess.

**M5. `Workspace::destroy()` takes only `session_id` but needs `repo_path` and `branch`.**

The `Workspace::destroy()` signature is `async fn destroy(&self, session_id: &str) -> Result<(), WorkspaceError>` (ADR Section 2, line 123). The worktree implementation needs to run `git -C {repo_path} worktree remove` and `git -C {repo_path} branch -D {branch}`. Both `repo_path` and `branch` are not available from `session_id` alone.

The `WorktreeWorkspace` is constructed with `config` and `paths` (factory, line 151), so it can derive `worktree_path` from `DataPaths::worktree_path(session_id)` and `repo_path` from config. But `branch` is not derivable from `session_id` — it depends on the naming convention used during creation.

The design doc (Section 2, line 105) describes `destroy()` as running `git branch -D {branch}`, but the branch name is only available in `WorkspaceCreateContext.branch` (at creation time) or in the metadata file (`BRANCH` field). The Workspace trait has no access to SessionStore.

**Evidence:** ADR Section 2, line 123 (destroy signature); design doc Section 2, line 105 (branch deletion in destroy).

**Recommendation:** Either (a) add a `branch: &str` parameter to `destroy()`, (b) derive the branch name from a convention (e.g., branch name = session_id), or (c) have the workspace implementation query `git worktree list --porcelain` to find the branch associated with the worktree path. Option (c) is the most robust since it does not depend on naming conventions.

**M6. No `Workspace::info()` or `Workspace::get()` method for single-session lookup.**

The trait provides `list()` and `exists()` but no way to retrieve `WorkspaceInfo` for a single session without calling `list()` and filtering. During the gather phase, the lifecycle engine needs to know the workspace path for a specific session. It can use `DataPaths::worktree_path()` directly, but this bypasses the Workspace trait abstraction — the `clone` plugin might store workspaces in a different location.

**Evidence:** ADR Section 2, lines 119-126 (Workspace trait — no single-session retrieval).

**Recommendation:** Add `async fn info(&self, session_id: &str) -> Result<Option<WorkspaceInfo>, WorkspaceError>` to the trait. This is a natural complement to `exists()` and avoids the N+1 query pattern of calling `list()` to find one session.

### Low

**L1. Archive timestamp format `YYYYMMDDTHHMMSSz` is ambiguous about timezone.**

The archive directory uses `{id}-{YYYYMMDDTHHMMSSz}` (ADR Section 3, line 248; design doc Section 3, line 327). The trailing `z` suggests UTC, but the format is not ISO 8601 (which would be `YYYYMMDDTHHMMSSZ` with uppercase Z, or with offset like `+0000`). If the system uses local time instead of UTC, archives created in different timezones could collide or sort incorrectly.

**Recommendation:** Use ISO 8601 basic format explicitly: `{id}-{YYYYMMDD}T{HHMMSS}Z` (UTC). Document that all timestamps are UTC.

**L2. `SessionStore::exists()` returns `bool` while all other methods return `Result`.**

The `exists()` method (ADR Section 3, line 238) returns a bare `bool`. If the underlying `std::fs::metadata()` call fails for a reason other than "not found" (e.g., permission denied), the error is silently swallowed. Every other SessionStore method returns `Result<_, StoreError>`.

**Evidence:** ADR Section 3, line 238.

**Recommendation:** Change to `Result<bool, StoreError>` for consistency and to surface unexpected filesystem errors.

**L3. `symlinks: Vec<String>` in `WorkspaceCreateContext` uses `String` instead of `PathBuf`.**

The `symlinks` field is described as "repo-relative paths" (ADR Section 2, line 110). Using `String` loses the type-level distinction between a path and an arbitrary string. Since these are always paths (relative to the repo root), `Vec<PathBuf>` or a newtype `RepoRelativePath(PathBuf)` would be more idiomatic Rust.

**Recommendation:** Use `Vec<PathBuf>` for consistency with other path fields in the same struct, or introduce a `RepoRelativePath` newtype if path validation is desired at the type level.

**L4. `SessionMetadata` does not derive `Serialize`/`Deserialize` but `JournalEntry` does.**

`JournalEntry` uses `#[derive(Serialize, Deserialize)]` (ADR Section 3, line 207) for JSONL serialization. `SessionMetadata` (line 190) has no serde derives — it uses custom KEY=VALUE serialization. This asymmetry is intentional but undocumented. A future developer might attempt to add `#[derive(Serialize)]` to `SessionMetadata` expecting JSON output.

**Recommendation:** Add a doc comment to `SessionMetadata` explaining that it uses custom KEY=VALUE serialization rather than serde, and why.

**L5. The `beforeRemove` hook runs before `workspace.destroy()` but its `cwd` is the workspace path.**

The hook execution (ADR Section 4, line 265) sets `cwd = workspace_path`. The destruction sequence (Section 5, line 297) runs `beforeRemove` before `workspace.destroy()`, so the workspace still exists. This is correct. However, if the hook itself fails (e.g., nonzero exit), the ADR does not specify whether destruction proceeds or aborts. Aborting destruction on a hook failure would leave the session in a limbo state — not archived, not active.

**Recommendation:** Document that `beforeRemove` hook failures are logged but do not prevent workspace destruction or archival. Destruction must be best-effort to avoid leaked resources.

---

## Consistency with Prior ADRs

| ADR | Assessment |
|-----|-----------|
| **ADR-0001 (Lifecycle Engine)** | Good integration. The three-phase poll loop is correctly mapped: gather reads metadata + agent info, evaluate is pure, transition writes metadata + appends journal. Entry actions for terminal states (`destroyWorkspace()`) map to the destruction sequence. One concern: ADR-0001 lists `destroyWorkspace()` as an entry action for `cleanup` and `killed` (line 39), but ADR-0005's destruction sequence includes `session_store.archive()` — archival is not mentioned in ADR-0001's entry action list. This is likely an omission in ADR-0001 rather than a conflict, but should be reconciled. |
| **ADR-0003 (Configuration System)** | Good alignment. `ProjectConfig.symlinks` flows into `WorkspaceCreateContext.symlinks`. `ProjectConfig.hooks` (the `Hooks` struct with `after_create`, `before_run`, `after_run`, `before_remove`) maps to the hook ordering table. `AO_SESSION` and `AO_DATA_DIR` are set per-session as specified. The hash-based data directory scheme matches ADR-0003's specification. Gap: ADR-0003's `Hooks` struct uses `Option<String>` (single command per hook), but ADR-0005's hook ordering table shows "config `afterCreate` -> agent `AfterCreate` hooks" — the agent hooks come from ADR-0004's `workspace_hooks()`, not from config. This is correctly handled but worth a cross-reference note. |
| **ADR-0004 (Plugin System)** | Good alignment. The Workspace trait follows the static factory pattern. `AgentWorkspaceHook::AfterCreate` hooks run after config hooks as specified. `AgentSessionInfo` fields (branch, pr_url, tokens_in, tokens_out) map to `SessionMetadata` fields. Session ID format `{prefix}-{issueId}-{attempt}` is consistent. The `PluginMeta` method on the Workspace trait matches the pattern. One gap: ADR-0004 defines `AgentWorkspaceHook::PostToolUse` (line 221), which is an agent-specific hook installed into the workspace (e.g., Claude Code's post-tool-use script). ADR-0005's hook ordering table does not mention `PostToolUse` — it only covers `AfterCreate`. The `PostToolUse` hook is installed during workspace creation but is not a lifecycle hook (it runs during agent execution, not at a lifecycle boundary). This distinction should be documented. |

---

## MVP Scope Assessment

The MVP scope is appropriate:

- `worktree` workspace with symlinks, validation, and idempotent destroy
- `SessionStore` with KEY=VALUE metadata, JSONL journal, atomic write, race-free creation, archive
- `DataPaths` with hash-based directories and collision detection
- Hook orchestration in the lifecycle engine
- Unwind logic for creation failures

**Potentially missing from MVP:**
- Status validation (H1) — without this, a corrupted metadata file can break recovery silently
- Newline handling in KEY=VALUE values (H2) — a `TERMINATION_REASON` with a newline is a realistic scenario
- Hook timeout default (M4) — implementers need a concrete value

**Correctly deferred:**
- `clone` workspace plugin, `restore()`, archive GC, journal compaction, cross-session atomicity

---

## Verdict

**Recommendation: Accept with revisions.** Address H1 (status type safety), H2 (KEY=VALUE newline handling), and H3 (unwind uses DataPaths not metadata) before accepting. M-level findings can be addressed during implementation, though M5 (destroy needs branch name) will surface as a compile-time issue and should be resolved in the design phase.
