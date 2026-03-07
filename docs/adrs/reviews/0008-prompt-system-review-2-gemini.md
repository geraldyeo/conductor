# ADR-0008 Review — Round 2 (Gemini)

Reviewing: ADR-0008 Prompt System (Proposed, amended after round 1)
Reference: docs/plans/2026-03-07-prompt-system-design.md

## Round 1 Findings — Resolution Status

All round 1 findings verified as resolved:

- **C1 (Fence bypass):** Resolved — delimiter escaping added to sanitizer pipeline (step 4).
- **H1 (Division by zero):** Resolved — empty guard added at step 1 of pipeline.
- **H2 (Redundant tracker call):** Resolved — documented as intentional with post-MVP optimization note.
- **H3 (Skills from project path):** Resolved — documented that skills load from canonical project path, not worktree.
- **H4 (Continuation-only fields):** Resolved — templates document which fields they reference, layer contracts specified.
- **M1 (Uniform comment budget):** Acknowledged as known trade-off.
- **M2 (OrchestratorContext session_id):** Resolved — `session_id` added.
- **M3 (load_skills edge cases):** Resolved — `is_file()` check, hidden file skip, non-UTF-8 skip, Tera syntax validation.
- **M4 (ToolDefinition.parameters unvalidated):** Acknowledged — deferred (tools empty at MVP).

## New Findings

### High

**H1: Tera template injection via untrusted content containing `{{ }}` or `{% %}`.**

If `issue.body` contains Tera template syntax like `{{ config }}` or `{% if true %}injected{% endif %}`, Tera may evaluate these expressions during `render()`, leaking template context variables or executing arbitrary template logic.

**Status:** Reviewed and determined to be a **false positive**. Tera variable substitution is not recursive — variable values are substituted as opaque strings and never re-parsed. Confirmed via Tera documentation. The ADR has been amended with an explicit security note and a regression test has been added to the testing strategy.

### Medium

**M1: No mechanism to detect template regression when base prompt wording changes.**

The base prompt's injection defense instruction references specific fence tag names (`<issue-content>`, `<comment>`). If a template edit changes the tag names without updating the defense instruction, the defense becomes ineffective.

**Status:** Acknowledged — the `{# SYNC #}` comment approach from round 1 L3 is sufficient. Golden file tests catch unintentional template changes.

**M2: `ToolDefinition.parameters` could break markdown code fence in tools.tera if it contains triple backticks.**

If `tool.parameters` contains ` ``` `, the code fence in `layers/tools.tera` would be structurally broken.

**Status:** Tools are empty at MVP. Validate at construction when FR17 lands.

## Verdict

**Accept.** All critical and high findings from round 1 are resolved. The new H1 (Tera template injection) is a false positive — Tera does not re-evaluate variable output. The design is sound and ready for implementation.
