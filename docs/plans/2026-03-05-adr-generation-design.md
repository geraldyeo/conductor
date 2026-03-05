# Design: ADR Generation from PRD

## Summary

Migrate the existing single-file `docs/ADR.md` (6 draft ADRs) into individual files under `docs/adrs/`, using a hybrid ADR format. Add one new "Proposed" ADR for implementation language choice. This is the foundational layer — dependent ADRs (CLI framework, dashboard framework, test framework, etc.) will be added once the language decision is made.

## Decisions Made

- **Layered approach**: Start with foundational ADRs that gate other decisions; add dependent ADRs later.
- **Hybrid format**: Nygard's simplicity (Status, Context, Decision, Consequences) + MADR's "Considered Options" section.
- **Flat numbered files**: `docs/adrs/NNNN-kebab-title.md` with a README.md index.
- **All existing ADRs are Draft status** — not yet reviewed or finalized.

## File Structure

```
docs/adrs/
  README.md                                      <- index with status table
  0001-eight-slot-plugin-architecture.md
  0002-workspace-isolation-git-worktrees.md
  0003-terminal-multiplexing-tmux.md
  0004-event-driven-lifecycle-polling.md
  0005-rule-based-automated-reactions.md
  0006-local-file-based-persistence.md
  0007-implementation-language.md
```

`docs/ADR.md` becomes a redirect to `docs/adrs/`.

## ADR Format Template

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

## Foundational ADRs (7 total)

### ADR-0001: Eight Slot Plugin Architecture (Draft)
- Context: Need to support evolving agents and varied infrastructure without modifying core.
- Options: Monolithic adapters vs strict slot-based plugins vs generic middleware pipeline.
- Leaning: Slot-based — prevents vendor lock-in, clear extension points.

### ADR-0002: Workspace Isolation via Git Worktrees (Draft)
- Context: Parallel agents need isolated filesystems without the overhead of full clones.
- Options: Git worktree vs full clone vs container-based isolation.
- Leaning: Worktree — fast, low disk, native git support. Clone as fallback.

### ADR-0003: Terminal Multiplexing with tmux (Draft)
- Context: CLI agents need persistent sessions with human attach capability.
- Options: tmux vs screen vs direct process management vs Docker exec.
- Leaning: tmux — robust session management, survives disconnects, native attach.

### ADR-0004: Event-Driven Lifecycle Polling (Draft)
- Context: Agents, CI, and PRs change state asynchronously.
- Options: Webhook-driven vs centralized polling vs hybrid.
- Leaning: Polling — resilient, works with local sessions, simpler. Can evolve to hybrid.

### ADR-0005: Rule-Based Automated Reactions (Draft)
- Context: Not all events need human intervention; behavior should be configurable.
- Options: Hardcoded handlers vs declarative reaction rules vs user scripts.
- Leaning: Declarative rules — configurable per-project, supports retry/escalation.

### ADR-0006: Local File-Based Persistence (Draft)
- Context: Session state must survive restarts without external database dependencies.
- Options: Flat key=value files vs SQLite vs external database.
- Leaning: Flat files — zero deps, bash-readable, sufficient for expected volume.

### ADR-0007: Implementation Language (Proposed)
- Context: CLI-first tool managing tmux, git, and shell processes. Needs easy distribution.
- Options: Rust vs TypeScript/Node.js vs Go.
- Leaning: Rust — single binary, no runtime deps, strong type system (traits for plugins).
- This decision gates: CLI framework, config library, async runtime, web framework, test framework, monorepo structure.
