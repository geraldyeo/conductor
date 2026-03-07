# ADR-0006: Tracker Integration -- Review Round 2 (Codex)

**Reviewer:** Codex
**Date:** 2026-03-07
**ADR Status:** Proposed
**Round:** 2 (verifying round 1 fixes)
**Files reviewed:**
- `docs/adrs/0006-tracker-integration.md`
- `docs/plans/2026-03-07-tracker-integration-design.md`
- `docs/adrs/0006-tracker-integration-review-1-codex.md` (own round 1)
- `docs/adrs/0006-tracker-integration-review-1-gemini.md` (Gemini round 1)

---

## Round 1 High Finding Resolution

### H1 (Codex): Precedence contradiction 28 vs 29
**Status: Resolved.**
The ADR now consistently uses precedence 28 throughout. Line 8 references "precedence 28 (ADR-0001)" and line 12 quotes `defineGlobalEdge("cleanup", 28, ...)`. The internal contradiction is eliminated. The design doc section 7 also references precedence 28 consistently.

### H2 (Codex): Missing derives on data structs
**Status: Resolved.**
All data structs now carry explicit derive annotations:
- `Issue`: `#[derive(Debug, Clone, PartialEq)]` (ADR line 71, design doc line 56)
- `IssueContent`: `#[derive(Debug, Clone)]` (ADR line 81, design doc line 68)
- `IssueComment`: `#[derive(Debug, Clone)]` (ADR line 91, design doc line 76)
- `TrackerState`: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]` (ADR line 98, design doc line 101) -- note that `Hash` was also added, addressing round 1 L1.
- Design doc additionally shows `IssueUpdate`: `#[derive(Debug)]` (line 84) and `IssueCreate`: `#[derive(Debug)]` (line 92).

### H3 (Codex): TrackerError missing Error/Display
**Status: Resolved.**
`TrackerError` now uses `#[derive(Debug, thiserror::Error)]` with `#[error(...)]` annotations on each variant (ADR lines 164-178, design doc lines 181-195). This provides `Debug`, `Display`, and `std::error::Error` implementations via `thiserror`, consistent with ADR-0002's crate choices.

### H4 (Codex): Untyped IssueComment.created_at
**Status: Resolved (accepted as String with documentation).**
The field remains `String`, but a doc comment now specifies: "Timestamps use ISO 8601 format (e.g., '2026-03-07T10:00:00Z'). String type chosen for simplicity -- the Tracker returns whatever the external API provides. Downstream consumers parse as needed." (ADR lines 88-96, design doc lines 75-80). This follows the approach recommended in my round 1 finding ("If keeping String, document the expected format and which module is responsible for parsing"). The decision to keep `String` is reasonable at MVP -- the only consumer is the prompt system (ADR-0008), which will render timestamps as-is.

### H1 (Gemini): Same precedence issue
**Status: Resolved.** Same fix as Codex H1 above.

### H2 (Gemini): Missing gh startup validation
**Status: Resolved.**
The factory function `create_tracker()` now calls `tracker.validate()` before returning the boxed trait object (ADR lines 148-149). The ADR explicitly states: "`GitHubTracker::validate()` runs `gh auth status` at construction time and returns an error if `gh` is not installed or not authenticated. This follows ADR-0004's pattern." (ADR lines 158-159). The Consequences section reinforces this: "The factory function validates `gh` presence and auth at construction time (fail-fast)." (ADR line 338). This is a proper fail-fast check at startup.

### H3 (Gemini): active_states documentation
**Status: Resolved.**
A dedicated paragraph now explains the scope of `active_states` (ADR lines 138-138): "`classify_state()` only checks `terminal_states`. The `active_states` config field is not consumed at MVP -- it exists for FR5 (Scheduling), where the scheduler uses it to filter which issues are eligible for auto-dispatch. At MVP, only manual `ao spawn` creates sessions, so `active_states` has no consumer." The design doc has a parallel note (line 176). This directly addresses the "user confusion" concern.

---

## Round 1 Medium Finding Status

| Finding | Status | Notes |
|---------|--------|-------|
| M1 (Codex): Empty/whitespace title in `branch_name()` | Resolved | ADR line 202: "If the title is empty or produces an empty slug after filtering, the branch name falls back to the issue ID only (e.g., `42`)." Design doc lines 234-235 show the implementation with explicit fallback. |
| M2 (Codex): UTF-8 truncation safety | Resolved | ADR line 202: "The slug is ASCII-only after the mapping step (all non-alphanumeric characters including multi-byte UTF-8 are replaced with hyphens), so byte-index truncation is safe." Design doc line 228 uses `is_ascii_alphanumeric()` (not `is_alphanumeric()`), and line 242 explains the safety invariant. |
| M3 (Codex): No `gh` startup check | Resolved | Elevated to H2 (Gemini) and addressed; see above. |
| M4 (Codex): `active_states` ignored | Resolved | Addressed together with Gemini H3; see above. |
| M5 (Codex): `repo` field duplication | Not addressed | Still a separate parameter in the factory signature. This remains a minor design tension but does not warrant elevation -- `repo` is a project-level concept, not a tracker concept, and the split is defensible. |
| M6 (Codex): `add_comment()` body with `--` prefix | Not addressed | No `--` end-of-flags sentinel documented. Low real-world risk since workpad comments are orchestrator-generated (not user input), but worth noting for post-MVP when FR4 reactions may generate comments from external data. Does not warrant elevation. |
| M1 (Gemini): Config divergence on hot-reload | Not addressed | `classify_state()` still takes `&TrackerConfig` separately from the tracker instance's stored config. Acceptable at MVP since hot-reload is post-MVP (ADR-0003). Does not warrant elevation. |
| M2 (Gemini): Branch collision handling | Not addressed | The ADR acknowledges collisions in Consequences (line 340) and notes `git worktree add` fails fast. The session creation sequence does not specify recovery behavior. Acceptable -- ADR-0005's unwind sequence handles the failure path. |
| M3 (Gemini): Typed timestamp | Resolved | Same as Codex H4; documented as ISO 8601 String. |
| M4 (Gemini): Body injection risk | Not addressed | Same scope as Codex M6. Acceptable at MVP. |
| M5 (Gemini): TrackerError Display/Error | Resolved | Same as Codex H3. |

---

## New Issues Introduced by Fixes

### N1 (Low): `validate()` error type mismatch

The factory function `create_tracker()` returns `Result<..., PluginError>`, and `tracker.validate()?` propagates via `?`. The design doc (line 174) states `validate()` "returns `PluginError`," which is consistent with the factory return type. However, `GitHubTracker` methods otherwise return `TrackerError`. The relationship between `PluginError` and `TrackerError` is not specified -- whether `PluginError` has a variant wrapping `TrackerError`, or whether `validate()` constructs `PluginError` directly.

This is a minor implementation detail that will be resolved when the error type hierarchy is implemented. No structural concern.

### N2 (Low): Stray backtick on line 159

ADR line 159 has a trailing triple backtick that appears to close a code block that was already closed, or is a formatting artifact. This is a minor Markdown issue:

```
`GitHubTracker::validate()` runs `gh auth status` at construction time...
```

The line sits between the factory code block (closed at line 156) and the error types section (starting at line 161). The stray backticks may render as an empty code block in some Markdown renderers.

---

## Remaining Medium Findings -- Elevation Assessment

None of the unaddressed medium findings from round 1 warrant elevation to High:

- **M5/M6 (Codex)** and **M1/M2/M4 (Gemini)** are implementation-time concerns that do not affect the architectural soundness of the ADR. They involve edge cases (branch collisions, `--` in comment bodies, config divergence under hot-reload) that are either handled by downstream mechanisms (ADR-0005 unwind, ADR-0003 hot-reload) or are low-probability at MVP scale.

---

## Verdict

**Accept.**

All seven High findings from round 1 (four from Codex, three from Gemini) have been addressed. The fixes are clean and do not introduce new structural issues. The two new observations (N1, N2) are Low severity -- one is an implementation detail, the other is a formatting artifact. The unaddressed Medium findings are acceptable for MVP and do not require changes before acceptance.

The ADR is ready for acceptance.
