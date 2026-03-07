# ADR-0002 Review — Round 1 (Codex)

**Reviewing:** ADR-0002: Implementation Language
**Design doc:** docs/plans/2026-03-06-implementation-language-design.md
**Reviewer:** Codex (simulated)
**Date:** 2026-03-06

## Strengths

1. **Decision framework is well-structured.** The three project-specific constraints (AI-as-implementer, 8-slot plugin architecture, graph-driven state machine) provide a clear lens for evaluation. Each option is assessed against these constraints rather than abstract "pros/cons."

2. **Honest treatment of Go.** The ADR does not strawman Go. It explicitly acknowledges Go's superiority for process management ergonomics and compile speed — the core domain activities — before arguing that type safety outweighs those advantages. This strengthens the credibility of the Rust choice.

3. **Landscape comparison adds context.** Citing ComposioHQ (TypeScript) and OpenAI/Symphony (Elixir) grounds the discussion in real-world precedent. The observation that neither chose Rust is stated forthrightly as a risk rather than hidden.

4. **Downstream decisions table is immediately actionable.** Locking in `clap`, `serde`, `tokio`, `axum`, `thiserror`/`anyhow`, and Cargo workspaces in a single decision point prevents downstream bikeshedding and lets implementation begin on a known foundation.

5. **Exhaustive matching argument is compelling.** For a system whose core is a 16-status, 30-transition state machine (ADR-0001), the compile-time guarantee that every status variant is handled is a genuine safety advantage. The design doc (line 41) concretely explains that adding a 17th status produces compile errors at every unhandled site.

6. **Elixir dismissal is well-reasoned.** The argument that OTP supervision trees solve a problem already addressed by Conductor's poll-and-converge model (design doc, line 85) is sound and avoids "shiny technology" bias.

## Findings

### Critical

None.

### High

**H1. Plugin system design needs more depth — `Box<dyn Trait>` is necessary but insufficient.**
The ADR states the plugin system uses "Traits + `Box<dyn Trait>`" (ADR line 48, design doc line 125) but does not address several Rust-specific complications:

- **Object safety constraints.** Not all trait designs are object-safe. If any plugin slot trait uses generics, associated types with complex bounds, or `Self`-returning methods, it cannot be used with `Box<dyn Trait>`. The ADR should commit to object-safe trait design or discuss alternatives (enum dispatch, generic monomorphization at config-load time).
- **Async trait methods.** As of 2026, `async fn` in `dyn Trait` requires either the `async_trait` crate (heap allocation per call) or nightly `async fn in traits` with `dyn`-compatibility constraints. The ADR references `tokio` but does not address how async plugin methods (e.g., `Agent::getActivityState()`, `SCM::detectPR()`) interact with dynamic dispatch. This is a practical implementation decision that should be stated.
- **No discussion of static vs dynamic plugin selection.** The ADR says "runtime plugin selection from config" but all plugins are known at compile time (they ship with the binary). An alternative is generic/enum dispatch resolved at startup, which avoids `dyn Trait` overhead and object-safety constraints entirely. The trade-off (compile-time monomorphization vs runtime flexibility for future external plugins) should be discussed.

*Recommendation:* Add a paragraph addressing object safety, async trait dispatch strategy, and whether external/third-party plugins are a goal. If all plugins are compiled-in, enum dispatch may be simpler and more performant.

**H2. Process/shell management ceremony is understated.**
The ADR acknowledges "more ceremony than Go" (ADR line 68, design doc line 50) but does not detail the specific friction points for this project's core domain:

- **tmux IPC.** Conductor's primary runtime shells out to `tmux` for session creation, message sending (`send-keys`), output capture (`capture-pane`), and liveness checks. In Rust, each of these is a `tokio::process::Command` call with `async`/`await`, error handling via `Result`, and output parsing. In Go, the equivalent is `exec.Command(...).Output()` — significantly less boilerplate. Given that tmux IPC is the most frequent operation in the system (every 30-second poll cycle for every session), this ceremony compounds.
- **Shell-out ergonomics for `gh` CLI.** The SCM plugin shells out to `gh pr view`, `gh pr checks`, `gh api graphql`, etc. Each call in Rust requires building a `Command`, awaiting it, checking the exit code, parsing stdout, and handling stderr. A helper function mitigates this, but the ADR should acknowledge that a `shell_out()` utility is a day-one requirement, not an afterthought.
- **`Send + Sync + 'static` bounds.** The ADR mentions these (design doc line 50) but does not explain the practical consequence: any data captured in an async task (including plugin trait objects) must satisfy these bounds. This constrains plugin trait design and is a source of confusing compiler errors for AI agents.

*Recommendation:* Add a mitigation section acknowledging that a `CommandRunner` or `ShellOut` utility abstraction is needed early to contain the ceremony. Acknowledge that `Send + Sync + 'static` bounds will require careful trait design.

### Medium

**M1. "No precedent" negative consequence lacks a mitigation strategy.**
The ADR lists "No precedent — no comparable agent orchestrator has been built in Rust" as a negative consequence (ADR line 71, design doc line 145) but offers no mitigation. This is not just an abstract risk — it means:

- No reference implementations to copy patterns from (unlike TypeScript, where ComposioHQ's patterns port directly).
- No community knowledge about Rust-specific pitfalls for this domain (e.g., tmux IPC patterns, GitHub API client ergonomics).
- Higher risk of architectural dead ends that would have been caught by prior art.

*Recommendation:* Add a mitigation: "The team will build proof-of-concept spikes for the highest-risk integration points (tmux session management, `gh` CLI wrapper, plugin trait hierarchy) before committing to full implementation. These spikes validate that Rust's async and trait system handle the core domain without excessive friction."

**M2. Downstream library choices should note alternatives and recency.**
The downstream decisions table (ADR lines 39-50) locks in specific libraries without noting alternatives or acknowledging the 2026 ecosystem:

- **`clap` vs `argh`**: `argh` is simpler and faster to compile. For a CLI with ~15 commands, `clap`'s feature richness may be unnecessary overhead.
- **`serde_yaml`**: The original `serde_yaml` crate (dtolnay) was archived in 2024. The community fork `serde_yml` or `yaml-rust2` are the maintained alternatives. The ADR should reference the maintained option.
- **`axum`**: Still the best choice in 2026, but worth noting that `loco.rs` (Rails-like framework built on axum) could accelerate dashboard API development.
- **Config validation**: The design doc mentions `garde` (line 119) but the ADR omits it. If config validation is a core requirement (FR10), the validation library should be listed.

*Recommendation:* Add a footnote or "Library Currency" note acknowledging that specific library versions will be validated during implementation setup, and update `serde_yaml` to its maintained successor.

**M3. Compile time mitigation is vague.**
The ADR says compile times are "mitigated by incremental builds and cargo workspaces" (ADR line 69). For AI agents doing rapid implement-compile-fix loops, compile time is a first-order concern. The ADR should be more specific:

- What is the expected clean build time for a project of this size? (Likely 30-90 seconds for a full rebuild, 5-15 seconds incremental.)
- Will `cargo check` (type-checking without codegen) be the primary feedback loop for AI agents?
- Should the workspace structure be designed to minimize recompilation (e.g., stable `core` crate that changes infrequently)?

*Recommendation:* Add a sentence: "AI agents should use `cargo check` as the primary feedback loop (sub-5-second type checking) and reserve `cargo build` for integration testing. Workspace crate boundaries should be drawn to minimize recompilation cascades."

**M4. Mobile companion app impact is dismissed too quickly.**
The ADR states "Mobile companion app is a fully separate codebase" (ADR line 67) as a negative consequence. FR9 specifies a mobile app with session cards, push notifications, and API communication. With TypeScript, this could share types and API client code via React Native. With Rust, the mobile app is a completely independent project with duplicated API types.

*Recommendation:* Acknowledge that the `axum` API should expose an OpenAPI spec (via `utoipa` or similar) to enable type generation for the mobile app and any future clients. This partially mitigates the code-sharing loss.

### Low

**L1. The ADR number in the filename (`0002`) does not match the convention example in AGENTS.md.**
AGENTS.md (line 51) shows `0007-implementation-language.md` as an example for this exact ADR topic. The actual file uses `0002`. This is cosmetic and likely reflects the example being written before the actual numbering was determined, but it could confuse automated tooling that cross-references ADR numbers.

*Recommendation:* No action needed — the `0002` numbering is correct for the actual sequence. Consider updating the AGENTS.md example to use a generic number like `NNNN`.

**L2. Design doc status says "Approved" (line 2) but ADR status says "Proposed" (line 4).**
These should be consistent. If the ADR is "Proposed" (awaiting review), the design doc should also say "Proposed" or "Draft."

*Recommendation:* Align the design doc status with the ADR status.

**L3. The ADR does not mention `tracing` for observability.**
Given that the lifecycle engine runs a 30-second poll loop with concurrent session checks, structured logging/tracing is critical for debugging. The `tracing` crate is the Rust ecosystem standard and integrates with `tokio`. It should be listed in the downstream decisions table alongside the other foundational crates.

*Recommendation:* Add `tracing` + `tracing-subscriber` to the downstream decisions table.

## Recommendation

**Accept with changes.** The core decision (Rust) is well-argued and the trade-off analysis is thorough. The findings above are refinements, not challenges to the decision itself. Before accepting:

1. Address **H1** by adding a paragraph on plugin trait design strategy (object safety, async dispatch, static vs dynamic).
2. Address **H2** by acknowledging the need for a shell-out utility abstraction as a day-one implementation task.
3. Address **M1** by committing to proof-of-concept spikes for the highest-risk Rust integration points.
4. Address **M2** by updating `serde_yaml` to its maintained successor and adding `garde` to the table.
