# CLAUDE.md Development Workflows — Design

**Date:** 2026-03-08
**Status:** Approved

## Problem

CLAUDE.md covers project architecture and review workflows but has no development workflows. With all 8 MVP ADRs accepted and the project entering the implementation phase, Claude and subagents need clear guidance on environment setup, build commands, and how to develop features in isolation with automated code review.

## Decisions

### Structure

Option C: self-contained "Development" section added directly to CLAUDE.md with four subsections. CLAUDE.md is loaded into every session; keeping workflows there means subagents always have the full process without extra file reads.

### Status Section Update

Update the Status section from "design/documentation phase" to reflect the transition to implementation.

---

## Section 1: Environment Setup

**Prerequisites** (manually installed):

- `rustup` with stable toolchain
- `cargo-nextest` (`cargo install cargo-nextest`)
- `gh` CLI (GitHub)
- `tmux`
- `gemini` and `codex` CLIs (for multi-model review)

**Bootstrap:** `make setup` — checks all prerequisites, installs missing Cargo tools, configures git hooks (pre-commit rustfmt check). Dashboard and mobile toolchains are TBD and handled separately.

---

## Section 2: Build & Test Commands

Single Cargo workspace at repo root covers `packages/cli` and `packages/core`.

| Command | Purpose |
|---|---|
| `cargo build --workspace` | Build all packages |
| `cargo nextest run` | Run all tests (preferred) |
| `cargo nextest run -p core` | Run tests for a specific package |
| `cargo clippy --workspace -- -D warnings` | Lint (warnings are errors) |
| `cargo fmt` | Format all code |
| `cargo fmt --check` | Check formatting (CI) |

`cargo-nextest` is preferred over `cargo test` for better output, parallelism, and retry support.

Dashboard and mobile have their own toolchains (TBD) and are not part of the Cargo workspace.

---

## Section 3: Feature Development Flow

Work is broken into independent tasks, each executed by a subagent in an isolated worktree.

**Steps:**

1. **Decompose** — break the feature into independent tasks (`TaskCreate` to track them)
2. **Parallelise** — use `superpowers:dispatching-parallel-agents` to spawn subagents for independent tasks simultaneously
3. **Per-task flow** (each subagent):
   - `EnterWorktree` to create an isolated workspace on a feature branch
   - Implement the task
   - Run the review loop (Section 4)
   - Open a PR from the worktree branch when review passes

**Branch naming:** `feat/<scope>/<short-description>` (e.g., `feat/core/session-store`)

**PR scope:** one PR per worktree/task — keeps diffs reviewable and review loops fast.

**Human role:** reviews and merges PRs. Subagents do not merge.

### Conflict Resolution

Task decomposition should assign non-overlapping file/module ownership per subagent. If two tasks touch the same file they are sequential, not parallel.

When conflicts occur after another PR merges:
1. The responsible subagent rebases its worktree branch onto updated `main`
2. Re-runs `/code-review-multi diff` on the rebase result
3. Updates the PR

PRs with no conflicts merge first. For complex features with many parallel PRs, a coordinator subagent can manage merge ordering and trigger rebases (applying the orchestrator-as-session pattern from ADR-0007 to development).

---

## Section 4: Review Loop

Each subagent runs this loop within its worktree before opening a PR:

1. Run `/code-review-multi diff` — dispatches Gemini + Codex in parallel, synthesises findings classified as Critical / Warning / Info
2. **Critical:** fix and re-run. Hard gate — no PR opens with an unresolved Critical.
3. **Warning:** fix and re-run. After 2 rounds, document remaining Warnings in the PR description with rationale. Humans decide in review.
4. **Info:** advisory only, no action required.
5. When no Critical findings remain → open PR.

**PR description must include:**
- Summary of what was implemented
- Any unresolved Warnings with rationale
- Review round count (e.g., "3 review rounds")

Mirrors the ADR review process (High findings gate acceptance; Medium/Low are noted) applied to code.
