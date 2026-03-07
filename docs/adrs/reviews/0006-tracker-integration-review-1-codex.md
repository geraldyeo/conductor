# ADR-0006: Tracker Integration -- Review Round 1 (Codex)

**Reviewer:** Codex
**Date:** 2026-03-07
**ADR Status:** Proposed
**Files reviewed:**
- `docs/adrs/0006-tracker-integration.md`
- `docs/plans/2026-03-07-tracker-integration-design.md`
- `docs/adrs/0001-session-lifecycle-engine.md` (dependency)
- `docs/adrs/0003-configuration-system.md` (dependency)
- `docs/adrs/0004-plugin-system-agent-runtime.md` (dependency)
- `docs/adrs/0005-workspace-session-metadata.md` (dependency)
- `docs/prds/0001-agent-orchestrator.md` (FR16, FR17, Section 5.3)

---

## Strengths

1. **Clean separation of concerns.** State classification is correctly extracted as a pure function outside the trait. This is the right call -- orchestrator policy (which states are terminal) does not belong in a tracker plugin. The `classify_state()` function is trivially testable and works across GitHub and Linear without modification.

2. **Structured data over formatted prompts.** Replacing `generatePrompt()` with `get_issue_content()` returning raw `IssueContent` is a strong design decision. It avoids coupling two independent concerns (data retrieval and prompt composition) and correctly defers sanitization to ADR-0008.

3. **Conservative error handling in gather phase.** The three-way match (success -> classify, `NotFound` -> terminal, other error -> active) follows ADR-0001's convergence principle faithfully. Treating deleted issues as terminal and API failures as active is the correct default in both directions.

4. **Pre-spawn validation.** Checking issue existence and state before creating any resources avoids the ADR-0005 unwind sequence (steps 2-8). This is a meaningful optimization that eliminates unnecessary filesystem and git operations.

5. **Minimal MVP surface.** Five required methods with clear single responsibilities. Post-MVP methods have default implementations returning `NotImplemented`. The trait is lean enough to implement for a new backend quickly, yet sufficient for the lifecycle engine's needs.

6. **Input validation against command injection.** Validating `issue_id` as a positive integer before shelling out, combined with `CommandRunner` passing arguments as `Vec<String>` (no shell interpolation), provides defense in depth.

7. **Trait-agnostic factory pattern.** Consistent with ADR-0004's static factory approach. The `create_tracker()` function mirrors `create_agent()` and `create_runtime()`.

---

## Findings

### High

**H1: Internal precedence contradiction in ADR-0006.**
ADR-0006 line 8 states the cleanup edge is "at precedence 28," but line 12 quotes `defineGlobalEdge("cleanup", 29, ctx => ctx.trackerState === "terminal")` with precedence 29. ADR-0001 (line 45) definitively assigns `trackerState === "terminal"` to precedence 28. The design doc section 7 also references "precedence 28."

The value 29 in the `defineGlobalEdge` call on line 12 contradicts both the ADR's own prose and the upstream ADR-0001. This is a copy error that could cause an incorrect implementation if someone codes directly from the pseudocode.

**Recommendation:** Change the `defineGlobalEdge` call on line 12 to use precedence 28, matching ADR-0001 and the ADR-0006 prose on line 8.

---

**H2: Missing derives on data structs.**
`Issue`, `IssueContent`, `IssueComment`, `IssueUpdate`, and `IssueCreate` lack standard Rust derives. At minimum, `Issue` needs `Debug` and `Clone` for logging and for use in pre-spawn validation (where the issue is fetched once and then its fields are read in multiple places). `IssueContent` and `IssueComment` similarly need `Debug` and `Clone`. `IssueUpdate` and `IssueCreate` need `Debug` for structured logging of post-MVP mutation calls.

ADR-0005 sets the precedent: `SessionMetadata` explicitly lists its derives (`Debug, Clone, PartialEq, Eq, Display, FromStr` via `strum`). ADR-0006 should follow the same rigor.

**Recommendation:** Add explicit derive annotations to all data structs. At minimum: `#[derive(Debug, Clone)]` for `Issue`, `IssueContent`, `IssueComment`; `#[derive(Debug)]` for `IssueUpdate`, `IssueCreate`. Consider `PartialEq` on `Issue` for test assertions.

---

**H3: `TrackerError` is missing `std::error::Error` impl and common derives.**
`TrackerError` lacks `#[derive(Debug)]` and has no mention of implementing `std::error::Error` or `std::fmt::Display`. Every other error type in the system (`PluginError`, `WorkspaceError`, `StoreError`, `ConfigError`) is implicitly expected to implement `Error` for use with `?` and `Result` propagation. Without `Error` + `Display`, `TrackerError` cannot be used with `anyhow`, `thiserror`, or standard error-handling patterns.

The `add_comment` failure logging on ADR-0006 line 244 (`tracing::warn!("Failed to post session comment: {e}")`) requires `Display` on `TrackerError`.

**Recommendation:** Either derive via `thiserror` (consistent with ADR-0002's crate choices) or explicitly note that `TrackerError` implements `Debug`, `Display`, and `std::error::Error`. Include `#[error(...)]` annotations for each variant.

---

**H4: `IssueComment.created_at` is a `String`, not a typed timestamp.**
`created_at` as a bare `String` provides no parsing guarantees. Different trackers may return different formats (GitHub uses ISO 8601 with `Z` suffix; Linear uses ISO 8601 with timezone offset). Downstream consumers (ADR-0008 prompt system, potential future sorting/filtering) would need to re-parse this string.

ADR-0005 uses `String` for `created_at` and `updated_at` in `SessionMetadata`, but those are serialized to a flat KEY=VALUE format where typed timestamps add complexity. `IssueComment` has no such constraint -- it lives in-memory only.

**Recommendation:** Use `chrono::DateTime<Utc>` (or `time::OffsetDateTime` if `chrono` is not in the dependency set) for `created_at`. If keeping `String`, document the expected format (ISO 8601) and which module is responsible for parsing.

---

### Medium

**M1: `branch_name()` does not handle empty or whitespace-only titles.**
The design doc (section 4, lines 203-213) shows the slugification logic, but does not address the edge case where `title` is empty or contains only non-alphanumeric characters. After slugification and `trim_matches('-')`, the slug would be empty, producing a branch name like `42-` (with trailing hyphen after format!) or just `42` (if the final `trim_end_matches` catches it).

The testing strategy table mentions "empty title" as a test case (design doc line 348), which is good, but the ADR itself should specify the fallback behavior.

**Recommendation:** Specify that an empty slug falls back to issue ID only (`42`) or a fixed suffix (`42-untitled`). Document this in the ADR's branch name generation section.

---

**M2: `branch_name()` truncation may split multi-byte UTF-8 characters.**
The slugification converts to lowercase and maps non-alphanumeric chars to hyphens, then slices at byte index 50 (`&slug[..50]`). Since the slug at this point contains only ASCII characters (alphanumeric + hyphens), byte slicing is safe. However, this safety guarantee is implicit -- it depends on the mapping step having already replaced all non-ASCII characters with hyphens. The ADR should make this invariant explicit.

**Recommendation:** Add a brief note that the slug is ASCII-only after the `map` step, making byte-index truncation safe. Alternatively, use `.chars().take(50)` for robustness against future changes to the mapping logic.

---

**M3: No `gh` CLI availability check at startup.**
The Consequences section (line 316) mentions "Startup validation should check for `gh` presence and auth status" but this is stated as a should-do, not as part of the design. Neither the ADR nor the design doc specifies when or how this check runs, what error is returned, or whether it blocks startup.

ADR-0004 specifies "Startup validation checks all plugin names referenced in config are known, before any sessions are created" (line 63). The tracker plugin should follow the same pattern.

**Recommendation:** Add an explicit startup validation step: the factory function `create_tracker("github", ...)` or a separate `validate()` method checks `gh --version` and `gh auth status`. Failure returns a descriptive error that blocks orchestrator startup.

---

**M4: `classify_state()` only checks `terminal_states`, ignoring `active_states`.**
ADR-0003 defines `TrackerConfig` with both `active_states` and `terminal_states`. The PRD (FR16, line 325) specifies both as configurable. However, `classify_state()` only checks `terminal_states` and defaults everything else to `Active`.

This means `active_states` in the config is effectively dead configuration at MVP -- it is accepted by the config system but never read by the tracker. This is confusing for users who set `active_states` expecting it to matter.

If `active_states` is intentionally deferred (it is only needed for FR5 scheduling/auto-dispatch), the ADR should say so explicitly and the config system should consider not requiring it at MVP.

**Recommendation:** Either (a) document that `active_states` is unused at MVP and only consumed by FR5's scheduler, or (b) use `active_states` in `classify_state()` as a positive match (state in `active_states` -> Active, state in `terminal_states` -> Terminal, state in neither -> Active with a warning log).

---

**M5: `repo` field on `GitHubTracker` duplicates `TrackerConfig` data.**
`GitHubTracker` stores `repo: String` and `config: TrackerConfig` as separate fields. But `repo` comes from `ProjectConfig.repo`, not from `TrackerConfig`. The factory signature is `create_tracker(name, repo, config)` -- the `repo` parameter is passed alongside the config.

This is not wrong, but it means the tracker plugin receives data from two different config scopes (`ProjectConfig.repo` and `ProjectConfig.tracker`). If a future refactor moves `repo` into `TrackerConfig` or if the factory signature evolves, this split could cause confusion.

**Recommendation:** Consider whether `repo` should be part of `TrackerConfig` to keep the factory signature simpler: `create_tracker(name, config)`. If it must stay separate (because `repo` is a project-level concept, not a tracker concept), document the rationale.

---

**M6: `add_comment()` body is passed directly to `gh issue comment --body`.**
While `CommandRunner` uses `Vec<String>` arguments (preventing shell injection), the `body` content could still contain characters that interact with the `gh` CLI's argument parser. For example, a body starting with `--` could be misinterpreted as a flag.

**Recommendation:** Ensure the `add_comment` implementation uses `--` (end-of-flags sentinel) before positional arguments, or verify that `gh issue comment` handles arbitrary body content safely. Document this defensive measure.

---

### Low

**L1: `TrackerState` could benefit from `Hash` derive.**
`TrackerState` derives `Debug, Clone, Copy, PartialEq, Eq` but not `Hash`. If `TrackerState` is ever used as a HashMap key (e.g., for per-state concurrency limits in FR5), the missing `Hash` derive would require a change. Since the enum has only two unit variants, `Hash` is trivially derivable.

**Recommendation:** Add `Hash` to the derive list for future-proofing.

---

**L2: No `Serialize`/`Deserialize` on `TrackerState`.**
While `TrackerState` is currently used only in-memory (in `PollContext`), adding `serde` derives would enable structured logging, debug output, and potential future persistence without a breaking change.

**Recommendation:** Add `#[derive(Serialize, Deserialize)]` to `TrackerState`.

---

**L3: Design doc error mapping for `gh` exit code 4 is overloaded.**
The design doc (lines 223-224) maps `gh` exit code 4 to both `RateLimited` (HTTP 429) and `AuthFailed` (HTTP 401/403). The implementation must distinguish between these by parsing stderr or the HTTP status from `gh`'s error output. The ADR should note that exit code 4 is `gh`'s generic "HTTP error" code and that stderr parsing is required for differentiation.

**Recommendation:** Document the stderr parsing strategy for exit code 4 disambiguation. Consider whether a single `HttpError { status: u16, message: String }` variant would be cleaner than separate `RateLimited` and `AuthFailed` variants at the `TrackerError` level.

---

**L4: `issue_url()` hardcodes `https://github.com`.**
The `issue_url()` implementation returns `https://github.com/{repo}/issues/{issue_id}`. This breaks for GitHub Enterprise Server instances, which use custom domains. The Consequences section (line 307) mentions that `gh` handles Enterprise Server URLs, but `issue_url()` does not use `gh`.

**Recommendation:** Either derive the base URL from `gh` configuration (`gh api graphql --hostname` or environment variables), or document that `issue_url()` is GitHub.com-only at MVP with Enterprise support deferred.

---

**L5: Workpad comment format is not configurable.**
The ADR acknowledges this (line 320) as a post-MVP concern. The hardcoded format includes backtick-delimited session IDs and agent names, which is reasonable for GitHub-flavored Markdown but may not render well on all trackers. Since Linear is post-MVP, this is acceptable.

No action needed -- noted for completeness.

---

## Summary

The ADR is well-structured and demonstrates strong alignment with the dependency ADRs (0001, 0003, 0004, 0005). The trait design is minimal, the error handling follows the convergence principle, and the separation between data retrieval and prompt composition is clean.

The highest-priority fixes are:
1. **H1**: Fix the precedence value contradiction (28 vs 29) -- a straightforward text correction.
2. **H2-H3**: Add standard Rust derives and `Error`/`Display` implementations to all types -- necessary for the code to compile correctly in the broader system.
3. **H4**: Decide on typed vs string timestamps for `IssueComment.created_at`.

The medium findings (M1-M6) address edge cases and documentation gaps that would surface during implementation. None require structural changes to the design.
