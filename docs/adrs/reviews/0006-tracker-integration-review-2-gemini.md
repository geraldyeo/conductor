# ADR-0006: Tracker Integration -- Review Round 2 (Gemini)

**Reviewing:** `docs/adrs/0006-tracker-integration.md` (Status: Proposed)
**Design doc:** `docs/plans/2026-03-07-tracker-integration-design.md`
**Round 1 reviews:** Gemini (3 High, 5 Medium, 4 Low), Codex (4 High, 6 Medium, 5 Low)
**Date:** 2026-03-07

---

## Round 1 High Finding Resolution

### Gemini H1: Precedence inconsistency 28 vs 29

**Status: Resolved.**

The ADR now consistently uses precedence 28 throughout. Line 8 says "precedence 28 (ADR-0001)" and line 12 quotes `defineGlobalEdge("cleanup", 28, ...)`. The design doc section 7 (line 358) also references "precedence 28." The internal contradiction is eliminated, and the value aligns with ADR-0001.

### Gemini H2: Missing `gh` startup validation

**Status: Resolved.**

The factory function `create_tracker()` now calls `tracker.validate()` after construction (ADR line 149), and the ADR explicitly states: "`GitHubTracker::validate()` runs `gh auth status` at construction time and returns an error if `gh` is not installed or not authenticated" (ADR lines 158-159). The design doc (lines 165, 174) mirrors this with the same pattern. This follows ADR-0004's startup validation precedent and provides fail-fast behavior.

### Gemini H3: `active_states` ignored without documentation

**Status: Resolved.**

A dedicated paragraph now appears after the `classify_state()` function (ADR lines 138-139): "`active_states` config field is not consumed at MVP -- it exists for FR5 (Scheduling), where the scheduler uses it to filter which issues are eligible for auto-dispatch. At MVP, only manual `ao spawn` creates sessions, so `active_states` has no consumer." The design doc (line 176) repeats this explanation. The scope is clearly documented and the potential for user confusion is addressed.

### Codex H1: Same precedence issue

**Status: Resolved.** (Same fix as Gemini H1.)

### Codex H2: Missing derives on data structs

**Status: Resolved.**

All data structs now carry explicit derive annotations:
- `Issue`: `#[derive(Debug, Clone, PartialEq)]` (ADR line 71, design doc line 56)
- `IssueContent`: `#[derive(Debug, Clone)]` (ADR line 81, design doc line 68)
- `IssueComment`: `#[derive(Debug, Clone)]` (ADR line 91, design doc line 76)
- `IssueUpdate`: `#[derive(Debug)]` (design doc line 84)
- `IssueCreate`: `#[derive(Debug)]` (design doc line 92)
- `TrackerState`: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]` (ADR line 98, design doc line 101)

Note: `IssueUpdate` and `IssueCreate` do not appear in the ADR's code blocks (they are post-MVP types), but they are properly annotated in the design doc. This is acceptable -- the ADR's code blocks show MVP types only.

### Codex H3: `TrackerError` missing `Error`/`Display`

**Status: Resolved.**

`TrackerError` now uses `#[derive(Debug, thiserror::Error)]` with `#[error(...)]` annotations on each variant (ADR lines 164-178, design doc lines 181-195). This provides `Debug`, `Display`, and `std::error::Error` implementations. The error is usable with `?`, `tracing::warn!("{e}")`, and standard Rust error-handling patterns.

### Codex H4: Untyped `IssueComment.created_at`

**Status: Resolved (with documentation).**

The ADR chose to keep `created_at` as `String` but added explicit documentation: "Timestamps use ISO 8601 format (e.g., '2026-03-07T10:00:00Z'). String type chosen for simplicity -- the Tracker returns whatever the external API provides. Downstream consumers parse as needed." (ADR lines 88-90, design doc lines 75-76). This is a reasonable MVP trade-off -- the format is documented, and the responsibility for parsing is clearly assigned to downstream consumers.

---

## Round 1 Medium Finding Check

Reviewing whether any unresolved Medium findings from round 1 should be elevated.

### Gemini M1: Config duplication between tracker instance and `classify_state()` parameter

**Status: Unchanged -- remains Medium.**

The pure function still takes `&TrackerConfig` separately from the tracker instance. The ADR does not document who is responsible for passing the current config. This remains a potential source of divergence under hot-reload, but it is an implementation detail that does not affect the ADR's architecture. No elevation needed.

### Gemini M2: `branch_name()` collision handling

**Status: Partially addressed.** The ADR's Consequences section (line 318) still notes that `git worktree add` would fail fast on collision, and this is unchanged. However, the design doc now includes the empty-slug fallback (line 234-235), which addresses Codex M1. Branch collision handling remains an implementation concern, not an architectural one. No elevation needed.

### Gemini M3 / Codex H4: `IssueComment.created_at` type

**Status: Resolved.** (See Codex H4 above.)

### Gemini M4: `add_comment()` body injection risk

**Status: Unchanged -- remains Medium.** The `Vec<String>` argument passing prevents shell injection, but markdown rendering risks (mentions, references) are not addressed. This is acceptable for MVP -- the only caller is the hardcoded workpad comment format, which uses backtick escaping.

### Gemini M5 / Codex H3: Missing `Display`/`Error` on `TrackerError`

**Status: Resolved.** (See Codex H3 above.)

### Codex M1: Empty/whitespace-only title in `branch_name()`

**Status: Resolved.** The ADR (line 202) and design doc (lines 234-235) now specify the fallback: "If the title is empty or produces an empty slug after filtering, the branch name falls back to the issue ID only (e.g., `42`)."

### Codex M2: UTF-8 truncation in `branch_name()`

**Status: Resolved.** Both the ADR (line 202) and design doc (lines 228, 242) now explicitly note that `is_ascii_alphanumeric()` (not `is_alphanumeric()`) is used, guaranteeing the slug is ASCII-only after the mapping step. The design doc adds: "uses `is_ascii_alphanumeric()` (not `is_alphanumeric()`) to ensure the slug is pure ASCII, making byte-index truncation safe."

### Codex M3: No `gh` CLI availability check at startup

**Status: Resolved.** (Same fix as Gemini H2.)

### Codex M4: `classify_state()` ignores `active_states`

**Status: Resolved.** (Same fix as Gemini H3.)

### Codex M5: `repo` field duplication

**Status: Unchanged -- remains Medium.** The `repo` parameter is still passed separately from `TrackerConfig` in the factory signature. The rationale (repo is a project-level concept, not a tracker concept) is implicit. No elevation needed -- this is a minor API design preference.

### Codex M6: `add_comment()` body `--` flag confusion

**Status: Unchanged -- remains Medium.** No end-of-flags sentinel is documented. This is an implementation detail that `CommandRunner` can handle. No elevation needed.

---

## New Issues Introduced by Fixes

### N1: `validate()` error type mismatch (Low)

The factory function returns `Result<Box<dyn Tracker>, PluginError>`, and `tracker.validate()?` uses `?` to propagate (ADR line 149). However, `validate()` is described as checking `gh auth status` -- if it returns a `TrackerError` (e.g., `AuthFailed`), the `?` operator would need a `From<TrackerError> for PluginError` implementation. The ADR states `validate()` "returns an error" and the design doc says "returns `PluginError`" (line 174), which resolves this -- `validate()` returns `PluginError` directly, not `TrackerError`. This is consistent but worth noting: `validate()` is a construction-time check that uses the plugin-level error type, while trait methods use `TrackerError`. The distinction is sound.

No action required -- this is a documentation observation, not a defect.

### N2: ADR line 159 has a stray triple-backtick (Low)

After the `create_tracker` code block closes at line 155, there is a stray closing triple-backtick on line 159 (```)  before the "Error types" section. This appears to be a formatting artifact.

**Recommendation:** Remove the stray closing fence on line 159. Minor formatting issue only.

---

## Cross-ADR Consistency Check (Round 2)

| Dependency | Status |
|-----------|--------|
| ADR-0001 global edge precedence | Consistent (28 throughout). |
| ADR-0001 PollContext | Consistent (`TrackerState` enum, noted as type change). |
| ADR-0001 gather phase ordering | Consistent (tracker is step 4 of 4). |
| ADR-0003 TrackerConfig | Consistent. `active_states` scope is now documented. |
| ADR-0004 plugin patterns | Consistent. Factory, `meta()`, `async_trait`, startup validation all match. |
| ADR-0005 SessionMetadata.issue_id | Consistent. |
| ADR-0005 TerminationReason::TrackerTerminal | Consistent. |
| ADR-0005 SessionStore::list() | Consistent. |
| PRD FR16 | All requirements addressed. |

---

## Verdict: Accept

All 7 High findings from round 1 (3 Gemini + 4 Codex) have been resolved. The precedence is consistent, derives are present, `TrackerError` uses `thiserror`, `gh` startup validation is specified, `active_states` scope is documented, `IssueComment.created_at` format is documented, and branch name edge cases are handled.

No Medium findings warrant elevation. The two new issues identified (N1, N2) are both Low severity -- N1 is an observation confirming the design is sound, and N2 is a minor formatting artifact.

The ADR is ready for acceptance.
