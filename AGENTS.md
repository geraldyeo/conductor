# AGENTS.md

Shared conventions for AI agents working in this repository. This file is agent-agnostic — it applies to Claude, Gemini, Codex, and any other AI contributor.

## Project

**Conductor** is an orchestration layer for AI coding agents. The design/documentation phase is complete (8 ADRs accepted). The **full MVP is merged** — all 10 CLI commands, the orchestrator daemon, poll-driven 16-state lifecycle, YAML config system, and IPC protocol are implemented. See `CLAUDE.md` for development workflow.

## Repository Structure

```
docs/
  prds/            Product requirements documents
  prds/reviews/    PRD review files
  adrs/            Architecture decision records
  adrs/reviews/    ADR review files
  adrs/archive/    Archived (superseded) ADRs
  plans/           Design documents and implementation plans
  e2e-tests/
    <suite>/       One folder per test suite (e.g. mvp/, reactions/)
      plan.md      Test plan for the suite
      YYYY-MM-DD-results.md   Dated result files (multiple runs allowed)
```

## Document Conventions

### PRDs (`docs/prds/`)

| File type | Pattern | Location | Example |
|-----------|---------|----------|---------|
| PRD | `NNNN-kebab-title.md` | `docs/prds/` | `0001-agent-orchestrator.md` |
| Review | `NNNN-kebab-title-review-{round}-{reviewer}.md` | `docs/prds/reviews/` | `reviews/0001-agent-orchestrator-review-1-gemini.md` |

- `NNNN` — sequential number matching the PRD being reviewed
- `{round}` — review iteration (1, 2, 3…). Increments when the PRD is updated and re-reviewed.
- `{reviewer}` — who performed the review (e.g., `gemini`, `codex`, `claude`, `human`)

**PRD front matter** — every PRD must include YAML front matter for version tracking:

```yaml
---
version: "1.2"
date: 2026-03-06
status: Draft
---
```

- **version**: `major.minor` — increment minor for review-driven updates, major for structural rewrites
- **date**: last modified date
- **status**: `Draft` | `Review` | `Accepted`
- Reviewers should reference the PRD version in their review header

### ADRs (`docs/adrs/`)

| File type | Pattern | Location | Example |
|-----------|---------|----------|---------|
| ADR | `NNNN-kebab-title.md` | `docs/adrs/` | `0002-implementation-language.md` |
| Review | `NNNN-kebab-title-review-{round}-{reviewer}.md` | `docs/adrs/reviews/` | `reviews/0002-implementation-language-review-1-codex.md` |

**ADR format** — hybrid Nygard + MADR:

```markdown
# ADR-NNNN: Title

## Status
Draft | Proposed | Accepted | Deprecated | Superseded by ADR-XXXX

## Context
What problem are we solving? What forces are at play?

## Considered Options
1. **Option A** -- description
2. **Option B** -- description

## Decision
What we chose and why. ("Pending" for Proposed ADRs.)

## Consequences
What becomes easier/harder. Both positive and negative.
```

**Status lifecycle:**
- **Draft** — initial write-up, not ready for review
- **Proposed** — ready for review (trigger review round)
- **Accepted** — reviews passed, decision finalized
- **Deprecated / Superseded** — no longer applicable

Reviews gate the Proposed → Accepted transition.

**Index:** `docs/adrs/README.md` contains a status table of all ADRs.

## Review Guidelines

When reviewing a PRD or ADR:

1. **Save your review** in the `reviews/` subdirectory following the naming convention above.
2. **Categorize findings** by severity: Critical, High, Medium, Low.
3. **Cite evidence** with file paths and line numbers where applicable.
4. **Separate strengths from gaps** — acknowledge what is sound before listing issues.
5. **Prioritize actionable recommendations** — state what should change, not just what's wrong.

## Commit Convention

Follows [Conventional Commits](https://www.conventionalcommits.org/) (Google style).

### Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Types

| Type | Purpose |
|------|---------|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `style` | Formatting, whitespace (no logic change) |
| `refactor` | Code change that neither fixes nor adds |
| `perf` | Performance improvement |
| `test` | Adding or updating tests |
| `build` | Build system or dependencies |
| `ci` | CI/CD configuration |
| `chore` | Maintenance tasks |
| `revert` | Reverts a previous commit |

### Scopes

Use the relevant area: `prd`, `adr`, `plan`, `cli`, `core`, `dashboard`, `mobile`, `config`, `plugin`, `workspace`, `agent`, `tracker`, `reaction`, `prompt`.

### Rules

- **Subject**: imperative mood, lowercase, no period, max 50 chars (e.g., `add budget caps to session config`)
- **Body**: wrap at 72 chars, explain "why" not "what", separate from subject with blank line
- **Breaking changes**: add `!` after type/scope (e.g., `feat(config)!: rename maxAgents to maxConcurrentAgents`) and include `BREAKING CHANGE:` in footer
- **Footer**: include co-author trailer (e.g., `Co-Authored-By: Gemini <noreply@google.com>`)
- Prefer small, focused commits over large batches

### Examples

```
docs(prd): add mutation authority model as FR17

Define split ownership between agent and orchestrator for tracker/PR
mutations. Agents own work-level actions, orchestrator owns lifecycle
actions. Enforced via tool withholding in FR11.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
```

```
docs(adr): add ADR-0007 implementation language

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
```
