# Session Lifecycle Engine ADR — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Write ADR-0001 for the graph-driven session lifecycle engine, get it reviewed, and mark it Accepted.

**Architecture:** The ADR formalizes the approved design (docs/plans/2026-03-06-session-lifecycle-engine-design.md) into the hybrid Nygard+MADR format defined in AGENTS.md. It covers the considered options (table-driven, state pattern, reducer, graph-driven) and the decision with its consequences.

**Tech Stack:** Markdown, git

---

### Task 1: Write ADR-0001

**Files:**
- Create: `docs/adrs/0001-session-lifecycle-engine.md`

**Step 1: Write the ADR**

Follow the format from `AGENTS.md:53-73` (hybrid Nygard + MADR). The ADR must include:

- **Status**: Proposed
- **Context**: The problem (16-state lifecycle, transition table, poll loop) and forces (auditability, testability, incremental delivery, crash recovery)
- **Considered Options**: Four options evaluated during brainstorming:
  1. Table-Driven State Machine — flat array of `{ from, to, guard, precedence }` tuples
  2. State Pattern (OOP) — 16 state classes with `evaluate()` methods
  3. Reducer Pattern (Functional) — single pure function with switch/match
  4. Graph-Driven State Machine — directed graph with nodes (statuses) and edges (guarded transitions)
- **Decision**: Graph-driven state machine. Summarize the 6 design sections from the approved design doc.
- **Consequences**: Positive and negative, pulled from the design doc's Consequences section.

Reference the design doc for full detail rather than duplicating all pseudocode.

**Step 2: Verify structure**

Manually check the ADR against the AGENTS.md format:
- Has `# ADR-0001: Title`
- Has all 5 sections: Status, Context, Considered Options, Decision, Consequences
- Status is `Proposed`

**Step 3: Commit**

```bash
git add docs/adrs/0001-session-lifecycle-engine.md
git commit -m "docs(adr): add ADR-0001 session lifecycle engine

Graph-driven state machine for session status transitions.
Nodes are statuses, edges are guarded transitions with precedence.
Covers graph structure, poll context, construction with validation,
poll loop, transition side effects, and MVP scope.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Update ADR Index

**Files:**
- Modify: `docs/adrs/README.md:17-18` (the index table)

**Step 1: Add ADR-0001 to the index table**

Add row:
```markdown
| [ADR-0001](0001-session-lifecycle-engine.md) | Session Lifecycle Engine | Proposed |
```

**Step 2: Commit**

```bash
git add docs/adrs/README.md
git commit -m "docs(adr): add ADR-0001 to index

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3: Get Reviews (Two Reviewers)

The ADR needs reviews to gate the Proposed -> Accepted transition (per AGENTS.md:81).

**Files:**
- Create: `docs/adrs/0001-session-lifecycle-engine-review-1-gemini.md`
- Create: `docs/adrs/0001-session-lifecycle-engine-review-1-codex.md`

**Step 1: Review round 1 — two independent reviewers**

Dispatch two parallel review agents (Gemini and Codex via their respective tools/APIs, or simulate with two independent review passes). Each review must:

1. Follow AGENTS.md review guidelines (categorize by severity, cite evidence, separate strengths from gaps, prioritize actionable recommendations)
2. Reference ADR-0001 and the design doc
3. Save as the appropriate review file

**Step 2: Commit reviews**

```bash
git add docs/adrs/0001-session-lifecycle-engine-review-1-gemini.md docs/adrs/0001-session-lifecycle-engine-review-1-codex.md
git commit -m "docs(adr): add round-1 reviews for ADR-0001

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 4: Address Review Feedback and Accept

**Files:**
- Modify: `docs/adrs/0001-session-lifecycle-engine.md`
- Modify: `docs/adrs/README.md`

**Step 1: Address any Critical/High findings**

If reviewers surface Critical or High issues, update the ADR to address them. Low/Medium items can be acknowledged without changes.

**Step 2: Update status to Accepted**

Change `## Status` from `Proposed` to `Accepted`.

**Step 3: Update index**

Change status in README.md table from `Proposed` to `Accepted`.

**Step 4: Commit**

```bash
git add docs/adrs/0001-session-lifecycle-engine.md docs/adrs/README.md
git commit -m "docs(adr): accept ADR-0001 session lifecycle engine

Address review feedback and mark as Accepted.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```
