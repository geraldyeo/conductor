# ADR-0008 Review — Round 2 (Codex)

Reviewing: ADR-0008 Prompt System (Proposed, amended after round 1)
Reference: docs/plans/2026-03-07-prompt-system-design.md

## Round 1 Findings — Resolution Status

All round 1 findings verified as resolved:

- **C1 (Division by zero):** Resolved — empty guard before comment budget distribution (step 1 + step 3).
- **H1 (Fence delimiter injection):** Resolved — delimiter escaping added as step 4 in sanitizer pipeline. Opening and closing tags both escaped.
- **H2 (Blocking I/O in async context):** Resolved — `load_skills` and `load_user_rules` changed to `async fn` using `tokio::fs`.
- **H3 (SanitizeConfig on PromptEngine):** Resolved — `SanitizeConfig` removed from `PromptEngine`, now standalone type.
- **H4 (ContinuationStyle #[non_exhaustive]):** Resolved — `#[non_exhaustive]` added to enum.
- **H5 (Missing injection defense in continuation):** Resolved — injection defense header added to `agent_continuation.tera`.
- **M1 (Comment render order):** Resolved — comments reversed to oldest-first after selection.
- **M2 (agentRulesFile path traversal):** Resolved — canonicalize + starts_with check, `RulesFileOutsideProject` error.
- **M3 (Issue comments absent from launch):** Resolved — `IssueContext.comments` added, rendered in `layers/context.tera`.
- **M4 (PromptEngine::new map_err):** Resolved — explicit `.map_err(PromptError::TemplateCompile)?`, note that `From<tera::Error>` is intentionally not implemented.

## New Findings

### Low

**L1: Two-call tracker design has no cache invalidation note for post-MVP optimization.**

The design documents the two-call pattern and notes "post-MVP optimization: combine into a single `gh issue view` call." If the combined call is implemented, the separate `IssueContent` type (body + comments only) becomes redundant — it should evolve into a combined type. This is a documentation nit for future implementors.

**L2: `SanitizeConfig::default()` is called at the spawn site — if multiple spawn paths emerge, the config construction will be duplicated.**

At MVP there is exactly one spawn path (step 6). Post-MVP, continuation with `FullPrompt` also calls `sanitize_issue_content()`. Consider a helper or constant for the default config.

## Verdict

**Accept.** All critical and high findings from round 1 are cleanly resolved. The amendments are well-integrated — no inconsistencies introduced. The design is ready for implementation. Low findings are informational only.
