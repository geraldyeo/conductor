# ADR-0002: Implementation Language

## Status
Accepted

## Context
The implementation language gates all downstream technical decisions: CLI framework, config library, async runtime, plugin system mechanics, web framework, test framework, and monorepo structure.

Three project-specific constraints shape the decision:

1. **AI agents are the primary implementers** (Claude Code, Gemini, Codex). Contributor pool size and learning curve are irrelevant. What matters is compile-time safety, iteration speed, and code quality feedback loops.
2. **The 8-slot plugin architecture** requires a natural mapping from slot interfaces to language-level constructs. Explicit conformance (a plugin must declare it implements a slot) is safer than implicit conformance (a plugin happens to have the right method signatures).
3. **The graph-driven state machine** (ADR-0001) has 16 statuses and 30 guarded transitions. Exhaustive pattern matching prevents an entire class of missed-case bugs.

The core domain is process orchestration: spawning tmux sessions, managing git worktrees, shelling out to `gh` CLI, and polling process liveness.

Comparable projects made varied language choices: ComposioHQ/agent-orchestrator chose TypeScript (40K LOC, 17 plugins, fast iteration), OpenAI/Symphony chose Elixir (OTP supervision trees, fault tolerance). Neither chose Rust or Go.

## Considered Options
1. **Rust** -- Traits map 1:1 to 8 plugin slots via explicit `impl Trait for Type`. Exhaustive `match` on enums forces every status variant to be handled. Single static binary. `tokio` for async polling, `clap` for CLI, `serde` for config/serialization, `axum` for dashboard API. Trade-offs: slower compile times than Go; dashboard requires separate TypeScript frontend; process/shell management has more ceremony (async bounds).

2. **TypeScript (Node.js)** -- Proven at this exact problem (ComposioHQ shipped 40K LOC with identical plugin architecture). Single language across CLI, dashboard, and potentially mobile. Largest ecosystem (`zod`, `commander`, `pnpm`). Trade-offs: requires Node.js on target machines; structural typing means implicit interface conformance (misspelled method compiles fine); no exhaustive pattern matching; runtime type errors possible.

3. **Elixir (BEAM/OTP)** -- OpenAI chose this for Symphony. OTP supervision trees provide native fault tolerance (crashed agent auto-restarts). Hot code reloading without stopping agents. Expressive pattern matching. Trade-offs: smallest ecosystem; dynamic typing with optional Dialyzer; AI agents produce less reliable Elixir; BEAM runtime must be bundled for distribution; Conductor's fault tolerance is already solved by poll-and-converge (OTP is compelling but redundant).

4. **Go** -- Purpose-built for the core domain (`os/exec`, goroutines, channels are first-class for process management). Target audience's native language (`gh`, `kubectl`, `terraform` are Go). Near-instant compile times accelerate AI agent iteration loops. Single binary. Trade-offs: no sum types or exhaustive matching (missed `switch` cases are silent bugs); implicit interfaces; `if err != nil` verbosity; less expressive type system for state machine invariants.

## Decision
Option 1: Rust. The state machine is the heart of the system. Exhaustive `match` on 16 statuses and explicit `impl Trait` conformance for 8 plugin slots provide compiler-enforced correctness that no other option matches.

With AI agents as implementers, the Rust compiler becomes the most reliable reviewer in the chain — catching ownership violations, missing match arms, and trait conformance errors before tests run.

Go was the closest alternative. Its process management ergonomics and compile speed are genuinely better for this domain. But the lack of exhaustive matching on a 16-variant state machine and implicit interface conformance for an 8-slot plugin system are risks that compound as the codebase grows — especially with AI agents writing the code.

The primary cost is a separate TypeScript frontend for the dashboard (React SPA served by `axum`). This is a well-understood pattern and does not affect the core architecture.

**AI agent iteration costs.** The compiler catches bugs but also generates friction — borrow checker fights, lifetime annotations, and `Send + Sync + 'static` bounds are the most common sources of AI-generated Rust compilation failures. This is accepted because: (1) `cargo check` runs in sub-5 seconds for incremental changes, providing a fast feedback loop; (2) workspace crate boundaries minimize recompilation; (3) the bugs caught (missing match arms, trait violations) are harder to find at runtime than the friction is to resolve at compile time.

**Process management ceremony.** The core domain (tmux IPC, `gh` shell-outs, git worktree management) involves hundreds of subprocess call sites. A `CommandRunner` utility abstraction is a day-one implementation task to contain the verbosity of `tokio::process::Command`. The tmux runtime plugin will shell out to the `tmux` binary via this utility — no tmux-specific Rust crate exists or is needed.

**Plugin system design.** All slot traits must be object-safe. Async methods use the `async-trait` crate (heap allocation per call, negligible vs I/O cost). All plugins ship with the binary; `Box<dyn Trait>` is chosen over enum dispatch for extensibility, with the option to migrate if object safety becomes a burden.

**No-precedent mitigation.** Proof-of-concept spikes for three integration points before full implementation: (1) tmux session management via `CommandRunner`, (2) `gh` CLI JSON parsing, (3) one plugin slot trait with async methods and `Box<dyn Trait>` dispatch.

**Downstream decisions unlocked:**

| Decision | Choice | Rationale |
|----------|--------|-----------|
| CLI framework | `clap` (derive) | Industry standard; derives from struct definitions |
| Config | `serde` + `serde_yml` | Zero-boilerplate deserialization (maintained successor to archived `serde_yaml`) |
| Config validation | `garde` or custom | Declarative validation on deserialized structs |
| Async runtime | `tokio` | De facto standard |
| Web framework | `axum` | Built on tokio/hyper; tower middleware |
| WebSocket | `axum` built-in (tokio-tungstenite) | Terminal-over-WebSocket (FR7) |
| SSE | `axum` built-in (`Sse` extractor) | Dashboard real-time updates (FR7) |
| Dashboard frontend | TypeScript (React SPA) | Served as static assets by axum |
| API spec | `utoipa` (OpenAPI) | Type generation for dashboard, mobile, future clients |
| Test framework | `#[test]` + `cargo-nextest` | Parallel execution; no external dependency |
| Monorepo | Cargo workspaces | `cli`, `core`, `dashboard-api`, `plugins/*` crates |
| Plugin system | Traits + `Box<dyn Trait>` + `async-trait` | Runtime plugin selection; async methods in dyn dispatch |
| Serialization | `serde` | Uniform across YAML, JSON, API |
| Error handling | `thiserror` + `anyhow` | Structured library errors; ergonomic app errors |
| Observability | `tracing` + `tracing-subscriber` | Structured logging for poll loop; integrates with tokio |

Reference `docs/plans/2026-03-06-implementation-language-design.md` for full analysis including landscape comparison and per-option detailed trade-offs.

## Consequences
Positive:

- The compiler catches the most bugs at compile time of any option — ownership, exhaustive matching, trait conformance, lifetime violations.
- Single binary distribution with zero runtime dependencies.
- Traits provide a type-safe, explicit plugin system that mirrors the 8-slot architecture.
- Cargo workspaces provide mature monorepo structure with per-crate testing and incremental compilation.
- Cross-compilation for Linux, macOS, and Windows. (Runtime dependencies — tmux, git worktrees — constrain actual platform support to Linux and macOS for MVP.)
- `serde` provides uniform serialization across YAML config, JSON metadata, and API responses.

Negative:

- Dashboard requires a separate TypeScript frontend build pipeline.
- Mobile companion app (FR9, post-MVP) is a fully separate codebase. Mitigated by OpenAPI spec generation via `utoipa`.
- Process/shell management has more ceremony than Go. Mitigated by `CommandRunner` utility as a day-one task.
- Compile times are slower than Go. Mitigated by `cargo check` as primary feedback loop and workspace crate boundaries.
- AI agents must satisfy borrow checker and async bounds, increasing per-cycle friction. Accepted because the bugs caught are harder to find than the friction is to resolve.
- Fewer ready-made integrations in the LLM/AI tooling ecosystem compared to TypeScript/Python.
- No precedent — no comparable agent orchestrator has been built in Rust. Mitigated by proof-of-concept spikes for highest-risk integration points.
