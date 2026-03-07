# ADR-0002 Review — Round 1 (Gemini)

**Reviewing:** ADR-0002: Implementation Language
**Design doc:** docs/plans/2026-03-06-implementation-language-design.md
**Reviewer:** Gemini (simulated)
**Date:** 2026-03-06

## Strengths

1. **Well-structured evaluation framework.** The ADR identifies three project-specific constraints (AI-as-implementer, plugin slot mapping, exhaustive state machine matching) and evaluates all four options against them consistently. This is significantly better than a vibes-based language choice.

2. **Honest acknowledgment of trade-offs.** The Consequences section lists six negative items, including the critical "no precedent" point (line 71, ADR). The design doc explicitly states Go's process management ergonomics are "genuinely better for this domain" (line 111, design doc). This intellectual honesty strengthens the decision.

3. **Comprehensive downstream decisions table.** The design doc's table (lines 115-127) covers 11 downstream choices with rationale for each, providing a clear implementation roadmap. The inclusion of `garde` for config validation (line 119, design doc) shows attention to the PRD's hot-reload config validation requirement (FR10).

4. **Good landscape analysis.** Citing ComposioHQ and Symphony with their actual language choices and outcomes grounds the decision in real-world evidence rather than theory.

5. **Fair treatment of Go.** Go is presented as a genuinely strong alternative rather than a strawman, with specific acknowledgment of its superiority in the core domain of process orchestration.

## Findings

### Critical

None.

### High

**H1: The "AI agents as implementers" framing overstates compiler safety and understates iteration speed.**

The ADR's central thesis is that Rust's compiler acts as "the most reliable reviewer in the chain" (line 31, ADR). This is true for the bug classes it catches (missing match arms, trait conformance, ownership). However, the framing underweights a critical counter-argument: AI agents producing Rust must also satisfy the borrow checker, lifetime annotations, and async `Send + Sync + 'static` bounds — all of which are the most common sources of AI-generated Rust compilation failures. The compiler catches bugs, but it also generates significantly more friction in the implement-compile-fix loop.

The design doc acknowledges Go's "near-instant compile times" (line 92) and notes AI agents "iterate faster with Go's sub-second builds" (line 93), but then dismisses this with a single sentence. A fair treatment would quantify the trade-off: how many additional compile-fix cycles does Rust require per feature, and does the safety net justify the slower loop?

**Recommendation:** Add a subsection to the design doc addressing the Rust-specific AI agent failure modes (borrow checker fights, lifetime annotation complexity, async bounds propagation). Acknowledge that the compile-time safety net comes with a measurably higher iteration cost per feature, and state why that trade-off is acceptable.

**H2: Process orchestration ergonomics — the core domain — are insufficiently weighted.**

The PRD's core domain is process orchestration: spawning tmux sessions (FR1, FR3), managing git worktrees (FR2), shelling out to `gh` CLI (FR3, FR16), polling process liveness (Section 5.4), executing shell hooks (FR2 four lifecycle hooks), and managing environment variables (FR10). Every one of these operations is a subprocess spawn followed by output parsing.

The ADR acknowledges Rust has "more ceremony than Go" for this (line 68, ADR) but treats it as a minor negative. In practice, Rust's `tokio::process::Command` requires `.spawn()?.wait_with_output().await?` with proper error propagation, output decoding, and async context — versus Go's `exec.Command().CombinedOutput()` in three lines. For a system where the majority of I/O is subprocess management, this ceremony compounds across hundreds of call sites.

**Recommendation:** Add a concrete code comparison (even pseudocode) showing the subprocess management pattern in Rust vs Go for a representative operation (e.g., `gh pr list --json`). Quantify the expected number of shell-out sites in the codebase. Explicitly state that the team accepts the higher per-callsite cost because the state machine safety outweighs it.

**H3: The downstream decisions table is missing WebSocket/real-time transport.**

The PRD specifies WebSocket terminal access (FR7: "Direct terminal access via WebSocket"), SSE for dashboard updates (FR7: "Server-Sent Events with 5-second polling intervals"), and a direct terminal WebSocket port (FR10: `directTerminalPort`). The downstream decisions table covers `axum` for the web framework but does not address the WebSocket library, terminal multiplexing protocol, or SSE implementation. These are non-trivial in Rust and affect the dashboard architecture.

**Recommendation:** Add rows to the downstream decisions table for: (1) WebSocket library (e.g., `tokio-tungstenite` or axum's built-in WebSocket support), (2) SSE implementation approach, (3) terminal-over-WebSocket protocol (e.g., raw PTY relay or xterm.js compatible protocol).

### Medium

**M1: No discussion of Rust ecosystem maturity for tmux IPC.**

The tmux runtime plugin must create sessions, send keys, capture panes, and parse output (FR1, FR6 `ao send` command). There is no established Rust crate for tmux IPC — this will likely require shelling out to the `tmux` CLI or implementing the tmux control mode protocol from scratch. Go faces the same challenge, but Go's `os/exec` makes the shell-out path trivially ergonomic. The ADR should acknowledge this and state the intended approach.

**Recommendation:** Add a brief note on the tmux integration strategy: shell-out to `tmux` binary via `tokio::process::Command`, or implement control mode (`tmux -CC`), or use a crate if one exists.

**M2: Mobile companion app is dismissed too quickly.**

The ADR lists "Mobile companion app is a fully separate codebase" as a negative (line 67, ADR) but does not explore alternatives. TypeScript (Option 2) would enable React Native code sharing with the dashboard. If the mobile app (FR9) is in the MVP critical path, this is a meaningful cost. If it is post-MVP (which the memory file suggests), this should be explicitly stated.

**Recommendation:** Add a sentence clarifying that FR9 (mobile app) is post-MVP per the critical path analysis, so the separate-codebase cost is deferred and acceptable.

**M3: `Box<dyn Trait>` plugin system lacks discussion of async trait limitations.**

The downstream decisions table specifies "Traits with dynamic dispatch (`Box<dyn Trait>`)" for the plugin system (line 125, design doc). As of Rust 1.75+, `async fn` in traits is stabilized, but `async fn` in `dyn Trait` still requires workarounds (e.g., `async-trait` crate or manual boxing). Since every plugin slot involves async operations (runtime creates tmux sessions, workspace creates worktrees, tracker calls APIs), this is a practical implementation concern.

**Recommendation:** Add a note on the async trait strategy: whether the project will use the `async-trait` crate, manual `Pin<Box<dyn Future>>` returns, or wait for native `dyn async Trait` stabilization.

**M4: Cross-compilation claim needs qualification.**

The Consequences section claims "Cross-compilation for Linux, macOS, and Windows" (line 61, ADR). However, Conductor depends on tmux (Linux/macOS only), `gh` CLI, and git worktrees. Windows support would require WSL or alternative runtime plugins. The cross-compilation benefit is real for the binary but overstated for the system.

**Recommendation:** Qualify the cross-compilation point: the Rust binary cross-compiles, but the system's runtime dependencies (tmux, git worktrees) constrain actual platform support to Linux and macOS for MVP.

### Low

**L1: Elixir evaluation could note that its pattern matching is not exhaustive by default.**

The Elixir section (line 75, design doc) lists "Pattern matching makes guard functions expressive and concise" as a strength, but Elixir's `case` statements are not compiler-enforced exhaustive — a missing clause raises a runtime `CaseClauseError`. This undermines the comparison against Rust's compile-time exhaustive matching. A brief clarification would sharpen the contrast.

**L2: The landscape table could include one more Rust-adjacent reference.**

The landscape table (lines 25-32, design doc) shows no Rust projects. Including a note about Rust CLI tools in the adjacent DevOps space (e.g., `ripgrep`, `delta`, `zoxide`) would strengthen the argument that Rust is viable for CLI/process tooling even if no orchestrator has used it.

**L3: Minor inconsistency in precedence numbers between ADR-0001 and PRD.**

ADR-0001 defines precedence bands 0-2 for global edges (line 45, ADR-0001), but the PRD Section 5.3 numbers transitions 1-30 with global edges at 28-30 (lines 434-436, PRD). The ADR-0002 decision about exhaustive matching references "30 guarded transitions" (line 13, ADR-0002). This is not an ADR-0002 issue per se, but the language choice ADR's argument rests on the state machine complexity — ensuring the precedence model is consistent strengthens that argument.

## Recommendation

**Accept with changes.** The core decision (Rust) is well-reasoned and defensible. The state machine safety argument is the strongest justification, and the honest treatment of Go as a close alternative is commendable. However, the three High findings should be addressed before acceptance:

1. Add a frank discussion of Rust-specific AI agent iteration costs (H1).
2. Acknowledge the subprocess management ceremony more concretely, with an explicit acceptance statement (H2).
3. Complete the downstream decisions table with WebSocket/SSE/terminal transport choices (H3).

The Medium findings (M1-M4) are recommended but not blocking.
