# ADR-0008 Review — Round 3 (Codex)

Reviewing: ADR-0008 Prompt System (Proposed, amended after rounds 1 and 2)

## Round 2 Amendment Verification

The key amendment under review is the Tera template injection safety documentation. It appears in two locations:

1. **ADR (line 181):** States that Tera variable substitution is not recursive, that template syntax within values is never re-parsed, and that a regression test verifies this invariant.

2. **Design doc (Section 6, "Tera Template Syntax in Untrusted Content"):** Provides more detail, cites the Tera documentation (`{{ "{{ hey }}" }}` outputs the literal string), explains the AST-compile-once / opaque-string-substitution model, and references the regression test in Section 12.

**Verification:**

- The claim is technically accurate. Tera (and all Jinja2-derived engines) parse templates into an AST at registration time. Variable values inserted via `tera::Context` are treated as opaque strings during `render()` — they are never re-parsed through the template engine. `{{ issue.body }}` containing `{{ secret }}` outputs the literal text `{{ secret }}`.
- The design doc correctly notes that autoescaping is disabled for `.tera` files by default, which is relevant context — autoescaping would HTML-encode `<` and `>` but is orthogonal to template re-evaluation safety.
- The regression test is specified in Section 12: "render a prompt where `issue.body` contains `{{ secret }}` and `{% if true %}injected{% endif %}` — assert both appear literally in the rendered output, not evaluated." This is a correct and sufficient test for the invariant.
- Both documents are internally consistent — the ADR summarizes, the design doc provides the full rationale and test specification. No contradictions.

**Assessment:** The amendment is accurate, well-placed, and the regression test covers the invariant. Clean.

## New Findings

None. No Critical or High issues found. The documents are internally consistent, all round 1 and round 2 findings remain resolved, and the Tera safety amendment introduces no new problems.

## Verdict

**Accept.** The ADR and design document are clean. All prior findings are resolved, the Tera template injection safety documentation is technically accurate, and the regression test specification is sufficient. Ready for implementation.
