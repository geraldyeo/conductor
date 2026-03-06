# ADR-0002: Workspace Isolation via Git Worktrees

## Status

Draft

## Context

Running multiple AI agents in parallel requires isolated filesystems — each agent needs its own working directory and branch without conflicting with others. Traditional `git clone` duplicates the entire repository, which is slow for large repos and consumes significant disk space. Branch switching within a single clone prevents parallelism entirely, since only one branch can be checked out at a time.

The workspace strategy must also support symlinks for shared configuration files (`.env`, `.claude`) and post-create hooks (e.g., dependency installation) to ensure agents can start working immediately after workspace creation.

## Considered Options

1. **Git worktree** — Uses `git worktree add` to create lightweight checkouts that share the underlying `.git` object store. Creation takes seconds and disk overhead is minimal (only working tree files are duplicated). Each worktree gets its own branch, with git enforcing that no two worktrees share the same branch. Native git support means standard tooling works without modification.

2. **Full git clone per session** — Creates a complete repository copy for each agent session, including a separate `.git` store. Provides maximum isolation — each clone is fully independent. However, it is slow for large repositories and consumes proportionally more disk space. The simpler mental model comes at a significant resource cost.

3. **Container-based isolation (Docker volumes)** — Runs each agent in a Docker container with its own filesystem. Provides the strongest isolation (OS-level), but introduces heavy overhead, requires a running Docker daemon, and complicates the "human attach" workflow that is central to the orchestrator's debugging experience.

## Decision

Option 1 (git worktree) as the default, with Option 2 (full clone) as a supported fallback via the `clone` workspace plugin.

Worktrees provide the best balance of speed, disk efficiency, and git-native isolation. The `clone` strategy exists for edge cases where worktree limitations apply — such as submodule edge cases or users who prefer full isolation.

Directory paths use hash-based namespacing (`~/.agent-orchestrator/{sha256-12chars}-{projectId}/`) to prevent collisions when multiple orchestrator instances manage the same repository from different configurations. `.origin` files detect hash collisions.

## Consequences

**Positive:**
- Near-instant workspace creation — agents can start working within seconds of being spawned.
- Low disk overhead enables running many parallel sessions without exhausting storage.
- Branch isolation is enforced by git itself (worktrees cannot share branches).
- Symlinks and post-create hooks integrate naturally into the worktree lifecycle.

**Negative:**
- Worktrees share the reflog and git index locks, creating potential contention under very high parallelism.
- Some git operations (e.g., `git gc`, `git prune`) affect all worktrees sharing the same `.git` store.
- Hash-based directory namespacing adds complexity to the workspace manager implementation.
