# Implementation Language — Design Document

**Date:** 2026-03-06
**Status:** Approved
**Scope:** ADR for the implementation language choice (PRD Section 8)

## Problem

The implementation language gates all downstream technical decisions: CLI framework, config library, async runtime, plugin system mechanics, web framework, test framework, and monorepo structure. The project cannot proceed to implementation without this choice.

Three project-specific constraints shape the decision:

1. **AI agents are the primary implementers.** Claude Code, Gemini, and Codex will write the code. Contributor pool size and learning curve are irrelevant. What matters is how well the compiler catches mistakes, how fast the implement-compile-fix loop runs, and how reliably agents produce correct code.

2. **The 8-slot plugin architecture** requires a natural mapping from slot interfaces to language-level constructs. Explicit conformance (a plugin must declare that it implements a slot) is safer than implicit conformance (a plugin happens to have the right method signatures).

3. **The graph-driven state machine** (ADR-0001) has 16 statuses and 30 guarded transitions. Exhaustive pattern matching — where the compiler rejects code that doesn't handle every variant — prevents an entire class of bugs.

A secondary constraint is that the core domain is process orchestration: spawning tmux sessions, managing git worktrees, shelling out to `gh` CLI, and polling process liveness.

## Landscape

Comparable projects in the agent orchestration space have made varied choices:

| Project | Language | Rationale |
|---------|----------|-----------|
| ComposioHQ/agent-orchestrator | TypeScript | Fast iteration; 40K LOC shipped in 8 days; pnpm monorepo; 17 plugins |
| OpenAI/Symphony | Elixir (BEAM/OTP) | Supervision trees for fault tolerance; hot code reload; spec-driven |
| johannesjo/parallel-code | TypeScript (Node.js) | GUI-focused; lighter-weight orchestration |
| Claude Code | TypeScript + React | Terminal agent (not orchestrator) |
| Codex CLI | TypeScript | Terminal agent (not orchestrator) |

The two direct inspirations for Conductor made opposite choices: TypeScript for iteration speed, Elixir for fault tolerance. Neither chose Rust or Go.

## Options Evaluated

### Option 1: Rust

**Strengths:**
- Traits map 1:1 to the 8 plugin slots. `impl Agent for ClaudeCode` is explicit — the compiler rejects a plugin that doesn't satisfy the full trait contract.
- `match` on enums is exhaustive. A 16-variant `SessionStatus` enum forces every guard function and handler to cover every case. Adding a 17th status produces compile errors at every unhandled site.
- Single static binary with zero runtime dependencies. `cargo build --release` produces a self-contained executable.
- The compiler acts as the most reliable reviewer: ownership, lifetimes, `Send + Sync` bounds, and exhaustive matching catch bugs before tests run. With AI agents as implementers, this safety net is more valuable than iteration speed.
- `tokio` (async runtime), `clap` (CLI), `serde` + `serde_yaml` (config), `axum` (web API) are all mature and battle-tested.
- Cargo workspaces provide monorepo structure with per-crate testing and incremental compilation.

**Weaknesses:**
- Slower compile times than Go (mitigated by incremental builds and cargo workspaces).
- Dashboard UI requires a separate TypeScript frontend (React SPA served by `axum`). No code sharing with the CLI beyond generated types.
- Process/shell management has more ceremony than Go: async task spawning requires `Send + Sync + 'static` bounds, and process management through `tokio::process` is verbose compared to Go's `os/exec`.
- The LLM/AI tooling ecosystem is Python/TypeScript-first; fewer ready-made integrations.
- No comparable agent orchestrator has chosen Rust.

### Option 2: TypeScript (Node.js)

**Strengths:**
- Proven at this exact problem. ComposioHQ shipped 40K LOC with 17 plugins and 3,288 tests in TypeScript. Their 8-slot plugin architecture is nearly identical to ours — patterns port directly.
- Single language across CLI, web dashboard, and potentially mobile (React Native). Maximum code sharing.
- Largest ecosystem. `zod` for config validation, `commander` for CLI, `pnpm` workspaces for monorepo.
- Fastest iteration for AI agents — TypeScript is the strongest language for AI-assisted development today.

**Weaknesses:**
- Requires Node.js on target machines. Single-binary workarounds (`bun compile`, `pkg`) exist but are immature.
- Structural typing means interfaces are implicit — a plugin can almost satisfy a slot interface (e.g., misspelled method name) and compile without error.
- Types are erased at runtime. Despite compile-time checks, runtime type errors are possible. `zod` adds runtime validation but is a separate layer.
- No exhaustive pattern matching. TypeScript's `switch` with a `never` default is a workaround, not compiler-enforced.
- Process/shell management is less ergonomic than Go or Rust.

### Option 3: Elixir (BEAM/OTP)

**Strengths:**
- OpenAI chose Elixir for Symphony, validating it for agent orchestration.
- OTP supervision trees provide native fault tolerance. A crashed agent process is automatically restarted with error context while other agents continue unaffected.
- Hot code reloading without stopping running agents — a natural fit for hot-reload config (FR10).
- Pattern matching makes guard functions expressive and concise (though not compiler-enforced exhaustive — a missing `case` clause raises a runtime `CaseClauseError`, unlike Rust's compile-time rejection).
- Lightweight BEAM processes (millions concurrent) are future-proof for scaling beyond dozens of sessions.

**Weaknesses:**
- Smallest ecosystem and contributor pool of the four options.
- Dynamic typing with optional Dialyzer analysis is weaker than Rust's or TypeScript's compile-time guarantees.
- AI agents produce less reliable Elixir than TypeScript or Rust — fewer training examples, more idiomatic patterns to get wrong.
- Distribution requires bundling the BEAM runtime. Burrito and Bakeware exist but are less polished than Rust's single binary or Go's single binary.
- Mix umbrella projects are less mature than cargo workspaces or pnpm workspaces for monorepo structure.
- Least familiar to the DevOps/platform engineer target audience.
- Conductor's fault tolerance model is already designed: the poll loop gathers state, the graph evaluates, crash recovery is "re-poll on restart." OTP supervision trees are compelling but solve a problem we've already addressed with a simpler convergence model.

### Option 4: Go

**Strengths:**
- Purpose-built for the core domain. `os/exec`, goroutines, and channels are first-class for process management and shell orchestration. This is Go's home turf.
- The target audience's native language. `gh` CLI, `kubectl`, `terraform`, `docker` are all Go. Contributing a plugin feels natural.
- Near-instant compile times. AI agents doing implement-compile-fix loops iterate faster with Go's sub-second builds than Rust's multi-second rebuilds.
- Single binary distribution with straightforward cross-compilation.
- `cobra` + `viper` for CLI + config are the industry standard for Go CLI tools.
- Goroutines are simpler than tokio — no async runtime, no function coloring, no `Send + Sync + 'static` bounds. The poll loop with bounded concurrency is N goroutines with a semaphore channel.

**Weaknesses:**
- No sum types or exhaustive matching. A missed case in a `switch` on session status is a silent bug. This is the single biggest concern for a system whose core is a 16-status state machine.
- Interfaces are implicit (structural). A plugin misspelling a method name won't produce a compile error pointing at the slot interface — it simply won't satisfy the interface, and the error surfaces at a different, less obvious location.
- `if err != nil` verbosity on every shell-out, API call, and file operation.
- Less expressive type system than Rust for modeling state machine invariants, guard function contracts, and plugin slot hierarchies.

## Decision: Rust

The state machine is the heart of the system. Exhaustive `match` on 16 statuses and explicit `impl Trait` conformance for 8 plugin slots provide compiler-enforced correctness that no other option matches.

With AI agents as implementers, the Rust compiler becomes the most reliable reviewer in the chain. Three agents implementing concurrently will produce bugs. Rust's compiler catches ownership violations, missing match arms, type mismatches, and trait conformance errors before tests run — before review agents even see the code.

The primary cost is a separate TypeScript frontend for the dashboard. This is a well-understood pattern (Rust API + React SPA) and does not affect the core architecture.

Go was the closest alternative. Its process management ergonomics and compile speed are genuinely better for this domain. But the lack of exhaustive matching on a 16-variant state machine and implicit interface conformance for an 8-slot plugin system are risks that compound as the codebase grows — especially when AI agents are the ones writing the code.

### AI Agent Iteration Costs

The compiler-as-safety-net argument has a cost: AI agents writing Rust must also satisfy the borrow checker, lifetime annotations, and async `Send + Sync + 'static` bounds. These are the most common sources of AI-generated Rust compilation failures. The compile-time safety net catches bugs, but it also generates more friction per implement-compile-fix cycle than TypeScript or Go.

This trade-off is accepted for three reasons:

1. **`cargo check` is the primary feedback loop.** Type checking without codegen runs in sub-5 seconds for incremental changes. AI agents should use `cargo check` for rapid iteration and reserve `cargo build` for integration testing.
2. **Workspace crate boundaries minimize recompilation.** A stable `core` crate (types, traits, state machine) that changes infrequently means plugin and CLI crate changes don't trigger full rebuilds.
3. **The bugs caught are harder to find than the friction caused.** A missed match arm or trait conformance gap in TypeScript or Go surfaces at runtime — potentially only under specific conditions. Borrow checker fights are frustrating but deterministic and always resolved before merge.

### Process Management Mitigation

The core domain — tmux IPC, `gh` CLI shell-outs, git worktree management, process liveness polling — involves hundreds of subprocess call sites. In Rust, each shell-out via `tokio::process::Command` requires `.spawn()?.wait_with_output().await?` with error propagation, output decoding, and async context. In Go, the equivalent is `exec.Command().CombinedOutput()` in three lines.

This ceremony is accepted but must be contained. A `CommandRunner` utility abstraction is a day-one implementation task:

```
// Pseudocode — the shell-out pattern used everywhere
async fn run(program: &str, args: &[&str]) -> Result<Output>
async fn run_json<T: DeserializeOwned>(program: &str, args: &[&str]) -> Result<T>
async fn run_silent(program: &str, args: &[&str]) -> Result<ExitStatus>
```

This utility handles: command building, async execution, exit code checking, stdout/stderr capture, JSON deserialization for `gh` CLI output, timeout enforcement, and structured error reporting. Every tmux, git, and `gh` call goes through it.

The tmux runtime plugin specifically will shell out to the `tmux` binary via this utility (e.g., `tmux new-session`, `tmux send-keys`, `tmux capture-pane`). There is no established Rust crate for tmux IPC, and implementing tmux control mode (`tmux -CC`) is unnecessary — the CLI shell-out approach is the same pattern ComposioHQ uses in TypeScript.

### Plugin System Design

The downstream decisions table specifies "Traits + `Box<dyn Trait>`" for runtime plugin selection. This requires attention to three Rust-specific concerns:

**Object safety.** All plugin slot traits must be object-safe (no generic methods, no `Self`-returning methods, no associated types with complex bounds). This constrains trait design but is achievable — the plugin interfaces defined in the PRD (FR3) use concrete types throughout.

**Async trait methods.** Every plugin slot involves async operations (runtime creates tmux sessions, workspace creates worktrees, tracker calls APIs). As of 2026, `async fn` in `dyn Trait` requires the `async-trait` crate (heap allocation per call) or manual `Pin<Box<dyn Future>>` returns. The `async-trait` crate is the pragmatic choice for MVP — the per-call heap allocation is negligible compared to the I/O cost of the operations themselves (subprocess spawns, API calls).

**Static vs dynamic dispatch.** All plugins ship with the binary — there are no external/third-party plugins in the foreseeable future. An alternative to `Box<dyn Trait>` is enum dispatch: a single enum with one variant per implementation, resolved at startup from config. This avoids object-safety constraints and virtual dispatch overhead. However, `Box<dyn Trait>` is more extensible if external plugins become a goal, and the overhead is negligible for this use case. The ADR commits to `Box<dyn Trait>` with the option to migrate to enum dispatch if object safety becomes a burden during implementation.

### Risk Mitigation: No Precedent

No comparable agent orchestrator has been built in Rust. This means no reference implementations to copy patterns from, no community knowledge about Rust-specific pitfalls for this domain, and higher risk of architectural dead ends.

Mitigation: proof-of-concept spikes for the three highest-risk integration points before committing to full implementation:

1. **tmux session management** — create session, send keys, capture pane output, check liveness via `CommandRunner`.
2. **`gh` CLI wrapper** — run `gh pr list --json`, parse output, handle auth errors.
3. **Plugin trait hierarchy** — define one slot trait (e.g., `Runtime`) with async methods, implement it for tmux, confirm object safety and `Box<dyn Runtime>` dispatch works.

These spikes validate that Rust's async and trait system handle the core domain without excessive friction.

## Downstream Decisions Unlocked

| Decision | Choice | Rationale |
|----------|--------|-----------|
| CLI framework | `clap` (derive macros) | Industry standard; derives from struct definitions |
| Config | `serde` + `serde_yml` | Zero-boilerplate deserialization (`serde_yml` is the maintained successor to the archived `serde_yaml`) |
| Config validation | `garde` or custom | Declarative validation on deserialized structs |
| Async runtime | `tokio` | De facto standard; mature, battle-tested |
| Web framework (dashboard API) | `axum` | Built on tokio/hyper; tower middleware ecosystem |
| WebSocket | `axum` built-in (tokio-tungstenite) | Terminal-over-WebSocket for FR7; axum has first-class WebSocket support |
| SSE | `axum` built-in (`Sse` extractor) | Dashboard real-time updates (FR7); native axum support, no extra crate |
| Dashboard frontend | TypeScript (React SPA) | Served as static assets by axum; shared types via codegen |
| API spec | `utoipa` (OpenAPI generation) | Enables type generation for dashboard frontend, mobile app, and future clients |
| Test framework | Built-in `#[test]` + `cargo-nextest` | Parallel test execution; no external dependency |
| Monorepo | Cargo workspaces | `cli`, `core`, `dashboard-api`, `plugins/*` crates |
| Plugin system | Traits + `Box<dyn Trait>` with `async-trait` | Runtime plugin selection; `async-trait` for async methods in dyn dispatch |
| Serialization | `serde` | Uniform across YAML config, JSON metadata, API responses |
| Error handling | `thiserror` (library) + `anyhow` (application) | Structured errors in libraries, ergonomic errors in binaries |
| Observability | `tracing` + `tracing-subscriber` | Structured logging for the 30s poll loop; integrates with tokio |

Specific library versions will be validated during implementation setup. Alternatives (e.g., `argh` over `clap` for faster compile times) may be reconsidered if compile-time overhead proves problematic.

## Consequences

**Positive:**
- The compiler catches the most bugs at compile time of any option — ownership, exhaustive matching, trait conformance, lifetime violations.
- Single binary distribution with zero runtime dependencies.
- Traits provide a type-safe, explicit plugin system that mirrors the 8-slot architecture naturally.
- Cargo workspaces provide mature monorepo structure with per-crate testing and incremental compilation.
- Cross-compilation for Linux, macOS, and Windows from a single codebase. (Note: the binary cross-compiles, but runtime dependencies — tmux, git worktrees — constrain actual platform support to Linux and macOS for MVP.)
- The `serde` ecosystem provides uniform serialization across YAML config, JSON metadata files, and API responses.

**Negative:**
- Dashboard requires a separate TypeScript frontend build pipeline.
- Mobile companion app (FR9, post-MVP) is a fully separate codebase. Mitigated by OpenAPI spec generation via `utoipa`, enabling type generation for any client.
- Process/shell management has more ceremony than Go (async bounds, pinning). Mitigated by a `CommandRunner` utility abstraction as a day-one implementation task (see Process Management Mitigation above).
- Compile times are slower than Go. Mitigated by: `cargo check` as the primary AI agent feedback loop (sub-5s incremental), workspace crate boundaries drawn to minimize recompilation cascades, and `cargo-nextest` for parallel test execution.
- Fewer ready-made integrations in the LLM/AI tooling ecosystem compared to TypeScript/Python.
- No precedent — no comparable agent orchestrator has been built in Rust. Mitigated by proof-of-concept spikes for the three highest-risk integration points before full implementation (see Risk Mitigation above).
