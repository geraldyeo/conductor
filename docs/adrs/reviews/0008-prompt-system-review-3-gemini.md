# ADR-0008 Review — Round 3 (Gemini)

Reviewing: ADR-0008 Prompt System (Proposed, amended after rounds 1 and 2)

## Round 2 Amendment Verification

The Tera template injection documentation has been added in two places:

1. **ADR (line 181):** States that Tera variable substitution is not recursive, that template syntax within values is never re-parsed, explains the AST-once / opaque-string-substitution rendering model, and notes a regression test. This is accurate. Tera (and all Jinja2-derived engines) parse templates into an AST at compile time; `render()` substitutes variable values as literal strings into the AST output. The claim is correct.

2. **Design doc (section 6, "Tera Template Syntax in Untrusted Content"):** Provides the same explanation with additional detail, citing Tera documentation (`{{ "{{ hey }}" }}` outputs the literal string `{{ hey }}`). Also references the regression test in section 12. The design doc's testing strategy (section 12) includes the specific test case: render a prompt where `issue.body` contains `{{ secret }}` and `{% if true %}injected{% endif %}`, assert both appear literally.

The documentation is technically accurate, internally consistent between ADR and design doc, and the regression test covers the invariant. Verified clean.

## New Findings

None. No new Critical or High issues identified. The amendments from round 2 are clean and introduce no regressions or contradictions with prior ADRs.

## Verdict

**Accept.** The ADR is ready for status change from Proposed to Accepted. The design is sound, all prior findings are resolved, and the Tera injection safety documentation is accurate and backed by a regression test.
