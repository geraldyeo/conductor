# CLAUDE.md Development Workflows Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a Development Workflows section to CLAUDE.md covering environment setup, build/test commands, subagent-driven feature flow, and the code review loop, and update the Status section to reflect the transition to implementation.

**Architecture:** All changes are edits to a single file (`CLAUDE.md`). Each task is a targeted Edit followed by a commit. No source code is touched.

**Tech Stack:** Markdown, git.

---

### Task 1: Update the Status section

**Files:**
- Modify: `CLAUDE.md:11-17`

**Step 1: Edit the Status section**

Find and replace the opening sentence:

Old:
```
This project is in the **design/documentation phase**. No source code exists yet. Key design documents live in `docs/`:
```

New:
```
The design/documentation phase is complete (8 ADRs accepted). The project is entering the **implementation phase**. See [Development Workflows](#development-workflows) below for how to contribute code. Key design documents live in `docs/`:
```

**Step 2: Verify the edit**

Read `CLAUDE.md` lines 11–17 and confirm the new sentence is in place and the docs directory listing is unchanged.

**Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update status section — entering implementation phase"
```

---

### Task 2: Add Environment Setup subsection

**Files:**
- Modify: `CLAUDE.md` (append new section after the final line)

**Step 1: Append the Development Workflows header and Environment Setup subsection**

Add after the last line of the file:

```markdown

## Development Workflows

### Environment Setup

Run `make setup` to verify and install prerequisites. Requires:

- `rustup` (stable toolchain) — <https://rustup.rs>
- `gh` CLI — GitHub operations
- `tmux` — agent runtime
- `gemini` CLI — multi-model review
- `codex` CLI — multi-model review

Cargo tools (`cargo-nextest`) are installed by `make setup`. Dashboard and mobile toolchains are managed separately (TBD).
```

**Step 2: Verify**

Read the last 20 lines of `CLAUDE.md` and confirm the section header and bullet list are present and correctly formatted.

**Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add environment setup to dev workflows"
```

---

### Task 3: Add Build & Test subsection

**Files:**
- Modify: `CLAUDE.md` (append after Environment Setup)

**Step 1: Append the Build & Test subsection**

Add after the Environment Setup block:

```markdown

### Build & Test

Single Cargo workspace at repo root covers `packages/cli` and `packages/core`.

| Command | Purpose |
|---|---|
| `cargo build --workspace` | Build all packages |
| `cargo nextest run` | Run all tests (preferred over `cargo test`) |
| `cargo nextest run -p core` | Run tests for a specific package |
| `cargo clippy --workspace -- -D warnings` | Lint (warnings are errors) |
| `cargo fmt` | Format all code |
| `cargo fmt --check` | Check formatting (CI) |

`cargo-nextest` is preferred for better output, parallelism, and retry support. Dashboard and mobile have their own toolchains (TBD) and are not part of the Cargo workspace.
```

**Step 2: Verify**

Read the last 20 lines of `CLAUDE.md` and confirm the table is present and the note about nextest and dashboard/mobile is included.

**Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add build & test commands to dev workflows"
```

---

### Task 4: Add Feature Development Flow subsection

**Files:**
- Modify: `CLAUDE.md` (append after Build & Test)

**Step 1: Append the Feature Development Flow subsection**

Add after the Build & Test block:

```markdown

### Feature Development Flow

Work is broken into independent tasks, each executed by a subagent in an isolated worktree. Use `superpowers:dispatching-parallel-agents` to parallelise independent tasks.

**Per-task flow:**

1. `EnterWorktree` — create an isolated workspace on a feature branch (`feat/<scope>/<short-description>`, e.g. `feat/core/session-store`)
2. Implement the task
3. Run the [Review Loop](#review-loop) below
4. Open a PR when review passes — one PR per worktree/task

**Humans review and merge PRs. Subagents do not merge.**

#### Conflict Resolution

Assign non-overlapping file/module ownership per task. If two tasks touch the same file, sequence them rather than parallelise.

When conflicts arise after another PR merges:

1. Rebase the worktree branch onto updated `main`
2. Re-run `/code-review-multi diff` on the rebase result
3. Update the PR

PRs with no conflicts merge first. For features with many parallel PRs, a coordinator subagent can manage merge ordering (applying the orchestrator-as-session pattern from ADR-0007 to development).
```

**Step 2: Verify**

Read the last 30 lines of `CLAUDE.md` and confirm the numbered flow, branch naming convention, conflict resolution steps, and the coordinator note are all present.

**Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add feature development flow to dev workflows"
```

---

### Task 5: Add Review Loop subsection

**Files:**
- Modify: `CLAUDE.md` (append after Feature Development Flow)

**Step 1: Append the Review Loop subsection**

Add after the Feature Development Flow block:

```markdown

### Review Loop

Run within each worktree before opening a PR:

1. Run `/code-review-multi diff` — dispatches Gemini + Codex in parallel, classifies findings as Critical / Warning / Info
2. **Critical:** fix and re-run. Hard gate — no PR opens with an unresolved Critical.
3. **Warning:** fix and re-run. After 2 rounds, document remaining Warnings in the PR description with rationale. Humans decide in review.
4. **Info:** advisory only, no action required.
5. Open PR when no Critical findings remain.

**Every PR description must include:**

- What was implemented
- Any unresolved Warnings with rationale
- Number of review rounds completed
```

**Step 2: Verify**

Read the last 20 lines of `CLAUDE.md` and confirm all five numbered steps, the hard gate note, and the PR description requirements are present.

**Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add review loop to dev workflows"
```
