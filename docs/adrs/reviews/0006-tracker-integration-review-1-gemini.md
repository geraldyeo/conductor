# ADR-0006: Tracker Integration — Review Round 1 (Gemini)

**Reviewing:** `docs/adrs/0006-tracker-integration.md` (Status: Proposed)
**Design doc:** `docs/plans/2026-03-07-tracker-integration-design.md`
**Date:** 2026-03-07

---

## Summary

ADR-0006 defines a thin Tracker trait with 5 MVP methods (plus 5 post-MVP with default `NotImplemented` returns), a GitHub implementation via `gh` CLI, state classification as a pure function, and integration points for the gather phase, session creation, and recovery. The design is well-structured, follows established plugin patterns from ADR-0004, and makes sound trade-offs for MVP scope. Several issues need attention, most notably a precedence number inconsistency and missing details around `gh` startup validation.

---

## Strengths

1. **Clean separation of concerns.** `classify_state()` as a pure function outside the trait is an excellent design choice. It keeps tracker implementations focused on data fetching and makes state classification trivially testable and config-driven. This will pay dividends when Linear lands post-MVP.

2. **Consistent plugin patterns.** The trait follows ADR-0004's conventions precisely: `meta()` for metadata, `async_trait`, `Send + Sync`, static factory with `match` dispatch. The factory signature (`create_tracker(name, repo, config)`) mirrors `create_agent()` and `create_runtime()`.

3. **`IssueContent` over `generatePrompt()`.** Deferring prompt formatting to ADR-0008 is the right call. It avoids coupling the tracker to prompt logic, keeps sanitization in one place, and makes `IssueContent` reusable for non-prompt purposes (e.g., CLI display).

4. **Error-to-state mapping in the gather phase.** The three-way match (`Ok` -> classify, `NotFound` -> Terminal, other errors -> Active) is well-reasoned. Deleted issues becoming terminal and transient errors defaulting to active follows ADR-0001's convergence principle correctly.

5. **Pre-spawn validation.** Checking issue state before creating any resources avoids the ADR-0005 unwind sequence entirely. This is a meaningful optimization that reduces complexity in the error path.

6. **Non-blocking workpad comment.** Logging the failure and continuing is the right pattern — a comment failure should never block session creation.

7. **Recovery via existing poll loop.** Reusing the gather phase for crash recovery instead of introducing a separate recovery codepath is elegant and consistent with ADR-0001's design.

8. **Thorough PRD mapping table.** The PRD Interface Mapping section (ADR lines 272-283) explicitly maps all 9 PRD methods, noting which are replaced, deferred, or split. This makes auditability straightforward.

---

## Findings

### High

**H1: Precedence number inconsistency within ADR-0006.**
The ADR contradicts itself on the cleanup edge precedence:
- Line 8: "drives the `cleanup` global edge at precedence 28 (ADR-0001)"
- Line 12: `defineGlobalEdge("cleanup", 29, ctx => ctx.trackerState === "terminal")`

ADR-0001 (line 45) says `trackerState === "terminal"` = 28. The PRD transition table (line 435) says precedence 29. These are two separate inconsistencies: (a) the internal contradiction within this ADR, and (b) the ADR-0001 vs. PRD mismatch that this ADR inherits.

**Recommendation:** Resolve the ADR-0001 vs. PRD precedence discrepancy first (likely by updating the PRD table, since ADR-0001 is the authoritative source post-acceptance). Then use a single consistent value throughout ADR-0006. At minimum, remove the internal contradiction.

**H2: No startup validation for `gh` CLI presence and auth.**
The Consequences section (line 316) acknowledges `gh` as an external dependency and states "Startup validation should check for `gh` presence and auth status." However, neither the ADR Decision section nor the design doc specifies when or how this validation occurs. There is no mention of it in the factory function, the session creation sequence, or the lifecycle engine startup.

**Recommendation:** Specify that `GitHubTracker::new()` (or `create_tracker()`) validates `gh` availability by running `gh auth status --json` and returns an error if `gh` is missing or unauthenticated. This is a fail-fast requirement — discovering `gh` is missing on the first poll tick is too late.

**H3: `classify_state()` ignores `active_states` entirely.**
The implementation only checks `terminal_states`:

```rust
if config.terminal_states.iter().any(|s| s.eq_ignore_ascii_case(issue_state)) {
    TrackerState::Terminal
} else {
    TrackerState::Active
}
```

The `TrackerConfig` from ADR-0003 (line 14 of ADR-0006) includes both `active_states` and `terminal_states`. If `active_states` is never checked, it serves no purpose in the config schema at MVP. This is not necessarily wrong (the "unmatched defaults to active" decision makes `active_states` redundant for classification), but it creates a confusing config contract: users configure `activeStates` expecting it to matter, but it is silently ignored.

**Recommendation:** Either (a) document explicitly that `active_states` is only used by FR5 (Scheduling) for dispatch filtering and is ignored by `classify_state()`, or (b) add a validation warning at config load time if `active_states` is configured but not yet used. The current silence risks user confusion.

### Medium

**M1: `TrackerConfig` passed to factory but also to `classify_state()` separately.**
`GitHubTracker::new(repo, config)` stores the `TrackerConfig`, but `classify_state()` takes `&TrackerConfig` as a separate parameter. This means the config exists in two places during the gather phase: inside the tracker instance and as a separate reference passed to the pure function. If config hot-reload lands (ADR-0003 post-MVP), these could diverge.

**Recommendation:** Consider whether `classify_state()` should read from the tracker's stored config (via a method like `tracker.classify_issue_state(&issue)`) or whether the separate pure function is intentional. If intentional, document that the caller is responsible for passing the current config and that the tracker's stored config is only used for CLI commands (repo, auth), not for state classification.

**M2: `branch_name()` lacks collision handling.**
The Consequences section (line 318) acknowledges potential collisions and notes "`git worktree add` would fail fast on collision." However, the session creation sequence in Section 4 (ADR line 234) derives the branch name before workspace creation but does not handle the case where the branch already exists (e.g., from a prior failed attempt that was not fully cleaned up, or from a manually created branch with the same name).

**Recommendation:** Document the expected behavior: if `git worktree add -b {branch}` fails because the branch exists, the workspace creation step fails, triggering the ADR-0005 unwind sequence. Alternatively, consider appending the attempt number to the branch name (`42-fix-login-bug-1`) to avoid collisions across retries.

**M3: `IssueComment.created_at` is a `String`, not a typed timestamp.**
Both `Issue` and `IssueComment` use `String` for temporal fields (`created_at`). ADR-0002 chose Rust for type safety, and `chrono::DateTime<Utc>` would provide compile-time guarantees. This is a minor inconsistency with the project's type-safety philosophy.

**Recommendation:** Consider using `chrono::DateTime<Utc>` (or `time::OffsetDateTime`) for `created_at`. If raw strings are preferred for simplicity at MVP, document the expected format (ISO 8601) so downstream consumers (ADR-0008 prompt system) can parse reliably.

**M4: `add_comment()` body injection risk.**
The `add_comment()` method passes the body to `gh issue comment --body {body}`. While `CommandRunner` uses `Vec<String>` (not shell interpolation), the body content could contain markdown that renders unexpectedly on GitHub (e.g., `@mentions`, `#references`, task lists). The workpad comment format (line 240) uses backtick-escaped values, which is good, but the `add_comment()` trait method is general-purpose.

**Recommendation:** Document that callers of `add_comment()` are responsible for content formatting. Consider whether the trait should accept structured comment data (like a `CommentBuilder`) rather than raw strings, to make formatting conventions explicit. This is low priority for MVP but worth noting for post-MVP when reactions (FR4) may generate comments.

**M5: Missing `Display` / `Error` trait implementations for `TrackerError`.**
The `TrackerError` enum is defined but the ADR and design doc do not mention `std::fmt::Display` or `std::error::Error` implementations. For the error to work with `?` operator and `tracing::warn!("Failed: {e}")` (line 244), it needs `Display`. ADR-0004 has the same gap with `RuntimeError` and `PluginError` but those were accepted — this is a consistency note.

**Recommendation:** Add `#[derive(Debug)]` and note that `Display` and `Error` (via `thiserror`) will be implemented. Minor, but ensures the pseudocode examples compile as written.

### Low

**L1: Design doc Section 4 error mapping relies on `gh` exit codes that are not formally documented.**
The error mapping (design doc lines 223-226) maps exit code 4 to HTTP 429 or 401/403, and exit code 1 with "not found" in stderr to `NotFound`. The `gh` CLI does not formally document exit codes as a stable API. Exit code behavior has changed across `gh` versions (e.g., exit code 4 was introduced in `gh` 2.x for HTTP errors).

**Recommendation:** Add a note that the error mapping should be verified against the pinned `gh` version in CI (already mentioned in Consequences line 317) and that integration tests should cover each error case.

**L2: `list_issues()` post-MVP signature takes no parameters.**
The trait declares `async fn list_issues(&self) -> Result<Vec<Issue>, TrackerError>`. FR5 (Scheduling) will need filtering by state, labels, assignees, etc. The parameterless signature will require a breaking change when `list_issues()` is implemented.

**Recommendation:** Consider a `ListIssuesFilter` parameter with all-optional fields now, even if the implementation is deferred. This avoids a trait signature change when FR5 lands. Alternatively, explicitly note this as a known future breaking change.

**L3: No mention of GitHub Enterprise Server URL handling.**
The Consequences section (line 307) mentions `gh` handles Enterprise Server URLs, but neither the ADR nor the design doc explains how. The `repo` field is `"owner/repo"` — does the `gh` CLI resolve the correct host from its auth config? If so, this is implicit behavior that should be documented.

**Recommendation:** Add a brief note that `gh` resolves the GitHub host from its authentication configuration (`gh auth status` shows the active account), and that Enterprise Server support requires `gh auth login --hostname <host>`.

**L4: `branch_name()` truncation may split multi-byte UTF-8 characters.**
The slugification converts to lowercase and replaces non-alphanumeric chars with hyphens, which handles most cases. However, `&slug[..50]` performs byte slicing. If the slug contains multi-byte characters (from non-ASCII titles that survive `to_lowercase()`), this could panic.

**Recommendation:** Use `slug.char_indices()` to find the correct truncation point, or note that the slug is guaranteed ASCII-only after the alphanumeric filter (since `c.is_alphanumeric()` for non-ASCII chars would pass through). Clarify which behavior is intended.

---

## Cross-ADR Consistency Check

| Dependency | Status |
|-----------|--------|
| ADR-0001 gather phase (step 4 of 4) | Consistent. Tracker is correctly placed as the last gather step. |
| ADR-0001 PollContext | Consistent. `trackerState` field type updated from string to enum, noted as a minor change. |
| ADR-0001 global edge | **Inconsistent** — precedence 28 vs. 29 (see H1). |
| ADR-0003 TrackerConfig | Partially consistent — `active_states` is accepted but ignored (see H3). |
| ADR-0004 plugin patterns | Consistent. Factory, `meta()`, `async_trait`, `Send + Sync` all match. |
| ADR-0004 PluginMeta | Consistent. `meta()` returns `PluginMeta`. |
| ADR-0005 SessionMetadata.issue_id | Consistent. Used in gather phase and session creation. |
| ADR-0005 TerminationReason::TrackerTerminal | Consistent. Referenced in recovery section. |
| ADR-0005 SessionStore::list() | Consistent. Used in recovery. |
| PRD FR16 | All 4 requirements addressed: multi-backend (trait), state mapping (classify_state), cleanup on terminal (gather phase + global edge), blocker awareness (deferred to FR5 — acceptable). |
| PRD FR17 | Correctly deferred. Token-scoped auth noted in deferred items. |

---

## Verdict

The ADR is well-designed and ready for acceptance after addressing:
- **H1** (precedence inconsistency) — must be fixed, as it affects the correctness of the lifecycle integration
- **H2** (startup validation) — must be specified, as it is a fail-fast requirement
- **H3** (active_states documentation) — should be clarified to avoid user confusion

The Medium and Low findings are quality improvements that can be addressed during implementation without blocking acceptance.
