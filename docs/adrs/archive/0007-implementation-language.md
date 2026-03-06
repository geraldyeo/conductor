# ADR-0007: Implementation Language

## Status

Proposed

## Context

The Agent Orchestrator is a CLI-first tool that manages tmux sessions, git worktrees, and shell processes. It needs to:

1. Ship as an easily distributable binary or package with minimal runtime dependencies.
2. Handle concurrent session polling efficiently (dozens of sessions checked in parallel).
3. Parse and validate YAML configuration files.
4. Interact with GitHub APIs (REST and GraphQL) for PR, CI, and review state.
5. Serve a web dashboard with real-time updates.

The choice of implementation language is the most consequential technical decision in the project. It gates all downstream choices: CLI framework, configuration library, async runtime, web framework, test framework, and monorepo structure.

## Considered Options

1. **Rust** — Compiles to a single static binary with no runtime dependency on the target machine. Strong concurrency support via the tokio async runtime. The CLI ecosystem is mature (clap for argument parsing, serde for serialization). Traits map naturally to the 8-slot plugin architecture defined in ADR-0001 — each slot becomes a trait, and plugins are concrete implementations. The type system catches entire classes of errors at compile time.

   The trade-offs are real: steeper learning curve for new contributors, longer compile times (mitigated by cargo workspaces and incremental compilation), and a smaller contributor pool compared to TypeScript or Go. The web dashboard would be a separate process (e.g., Axum serving a SPA or htmx-based UI), and a mobile companion app would be a fully separate codebase.

2. **TypeScript/Node.js** — The fastest iteration speed and the largest ecosystem. A single language can span the CLI, web dashboard (Next.js), and potentially the mobile app (React Native). Commander.js for CLI parsing, Zod for config validation. The upstream reference implementation (ComposioHQ/agent-orchestrator) uses this stack, providing a working example to draw from.

   The trade-off: requires Node.js installed on the target machine (not a single binary). TypeScript's type system is structural and erased at runtime, meaning runtime type errors are possible despite compile-time checks. Process management and shell interaction are less ergonomic than in Rust or Go.

3. **Go** — Compiles to a single binary like Rust, with a simpler concurrency model (goroutines and channels). Strong CLI ecosystem (cobra for commands, viper for config). The most common language for DevOps and infrastructure tooling, which aligns with the orchestrator's target audience.

   The trade-off: less expressive type system than Rust — no sum types (until recently), no generics (until Go 1.18), and error handling via return values is verbose. The plugin architecture would rely on interfaces, which are implicit and less discoverable than Rust's explicit trait implementations. Larger contributor pool than Rust for DevOps tools, but smaller than TypeScript overall.

## Decision

Pending. Leaning toward Rust.

The single-binary distribution, strong trait-based type system, and memory safety without garbage collection pauses make Rust well-suited for a tool that manages multiple concurrent processes and needs to be reliable. The steeper learning curve is accepted as a trade-off for long-term correctness and performance.

## Consequences

*The following consequences assume Rust is selected. They will be updated when the decision is finalized.*

**Positive:**
- Single binary distribution — end users install one file with no runtime dependencies.
- Traits provide a natural, type-safe plugin system that aligns with the 8-slot architecture (ADR-0001).
- Excellent performance for concurrent polling and process management.
- Memory safety without garbage collection pauses.
- Cross-compilation for Linux, macOS, and Windows from a single codebase.

**Negative:**
- Longer compile times, mitigated by cargo workspaces and incremental compilation.
- Smaller contributor pool — fewer developers are fluent in Rust compared to TypeScript or Go.
- The web dashboard requires a separate frontend build pipeline (cannot share code with the CLI).
- The mobile companion app would be a fully separate codebase (no code sharing with the CLI or dashboard).
- Steeper onboarding for new contributors unfamiliar with Rust's ownership model.

**Downstream decisions unlocked (if Rust):**
- CLI framework: clap
- Config: serde + serde_yaml
- Async runtime: tokio
- Test framework: built-in + cargo-nextest
- Web framework for dashboard API: axum or actix-web
- Plugin system: traits with dynamic dispatch or compile-time generics
