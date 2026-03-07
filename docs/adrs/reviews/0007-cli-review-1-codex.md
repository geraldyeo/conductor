# ADR-0007 Review — Round 1 (Codex)

## Summary

ADR-0007 is a well-structured and thorough design that correctly ties together ADRs 0001-0006 into a cohesive CLI surface. The IPC control plane with Unix domain socket is a sound architectural decision that establishes the orchestrator as the single coordinator and cleanly separates read-only from mutating operations. However, there are several gaps in cross-ADR consistency, missing PRD coverage, and a concurrency concern in the Orchestrator struct that should be addressed before acceptance.

## Verdict

Accept with conditions (address High findings; Medium findings are recommended but not blocking)

## Strengths

- The IPC control plane is elegantly designed. Unix domain socket for local IPC is the right call — it avoids port allocation, provides liveness detection via the socket file, and eliminates the need for a PID file. The length-prefixed JSON protocol is simple and sufficient.
- The command routing table is well-reasoned: read-only commands bypass IPC (work when orchestrator is down), mutating commands go through IPC (serialized access). This is a clean separation.
- The `ao send` delivery flow is faithful to the PRD's "send command intelligence" (busy detection, Ctrl-U clear, buffer strategy, 3-retry verification) while correctly delegating to ADR-0004's `detect_activity()` and `RuntimeStep::SendMessage/SendBuffer`.
- Project auto-resolution is pragmatic — single-project configs need zero flags, multi-project configs get CWD-based detection. The 4-step algorithm is clear and deterministic.
- The module structure cleanly separates concerns: `commands/`, `ipc/`, `output/`, `resolve.rs`, `error.rs`. The decision to place `Orchestrator` in `core` (not `cli`) is correct — it keeps the poll loop testable without CLI dependencies.
- The consequences section is honest and thorough, particularly the acknowledgment of the 30s kill delay and the staleness window for `ao status`.
- The deferred items table is well-justified with clear dependency chains (orchestrator-as-session depends on ADR-0008, restore depends on RestorePlan, etc.).

## Findings

### Critical

(None)

### High

- **H1: `Orchestrator` struct uses `&self` for mutable operations.** The `Orchestrator` struct (ADR line 123-130, design doc line 354-369) declares `run()`, `handle_spawn()`, `handle_send()`, `handle_kill()`, and `handle_cleanup()` as `&self` methods. However, `handle_spawn()` must mutate `stores` (to create sessions) and `plugins` state, and `handle_kill()` must set `manualKill` on a `PollContext`. With `Arc<Config>` and `HashMap` fields, this requires interior mutability. The design should specify the concurrency strategy: `RwLock<HashMap<...>>` for stores, or `Arc<Mutex<SessionStore>>` per project, or channels. Without this, implementors will face deadlocks or data races. **Recommendation:** Add a note specifying interior mutability strategy (e.g., `DashMap` or per-project `Mutex`) and document lock ordering to prevent deadlocks between the poll loop and IPC handlers.

- **H2: PRD `ao start` flags `--no-orchestrator` and `--rebuild` are neither mapped nor explicitly deferred.** The PRD (FR6 line 127) specifies `ao start` with flags `--no-dashboard`, `--no-orchestrator`, and `--rebuild`. ADR-0007 maps `--no-dashboard` (accepted as no-op at MVP, ADR line 165) but does not mention `--no-orchestrator` or `--rebuild` anywhere — not in the command spec, not in the deferred items table. `--no-orchestrator` is particularly relevant since ADR-0007 defers orchestrator-as-session (FR13), which means the MVP `ao start` already effectively runs in `--no-orchestrator` mode. This should be explicitly addressed: is the flag accepted as a no-op? Or is the concept collapsed because MVP always runs without the orchestrator agent? **Recommendation:** Add `--no-orchestrator` and `--rebuild` to the deferred items table with rationale, or document that they are intentionally omitted because the MVP process model makes them unnecessary.

- **H3: PRD `ao stop [project]` accepts an optional project argument; ADR `ao stop` does not.** The PRD (FR6 line 128) specifies `ao stop [project]`, implying per-project stop capability. ADR-0007's `ao stop` (line 166-167) sends a global `Stop` request with no project scoping. Since `ao start` manages all projects from one config, a projectless `ao stop` is consistent with the foreground process model. But this deviation from the PRD should be explicitly documented. **Recommendation:** Add a note in the PRD Interface Mapping section explaining that `ao stop` is global because the orchestrator is a single process managing all projects.

- **H4: `ao spawn` signature diverges from PRD without justification.** The PRD (FR6 line 130) specifies `ao spawn <project> [issue]` where `project` is a positional argument. ADR-0007 (line 168) specifies `ao spawn <issue> [--agent <name>] [--open]` where project is resolved via `-p`/auto-resolution. The PRD Interface Mapping (ADR line 243) notes this difference but does not explain the design rationale. The auto-resolution approach is arguably better, but the PRD divergence should be explicitly justified. **Recommendation:** Add a brief rationale for why project was moved from positional to auto-resolved (consistency with other project-scoped commands, ergonomics for single-project setups).

### Medium

- **M1: `ao status` displays fewer columns than the PRD specifies.** The PRD (FR6 line 129) lists `ao status` as showing "branch, PR, CI, review, activity, age, tokens." The design doc (line 237-243) shows only: session ID, status, issue, branch, age, tokens. Missing: PR URL/number, CI status, review status, activity state. These are important operational columns — a user running `ao status` wants to know at a glance whether CI passed and whether review is pending. **Recommendation:** Add PR, CI, and activity columns to the status table (review status can be inferred from session status). These are available from `SessionMetadata` fields (`pr_url`) and could be enriched from the last gathered `PollContext` if persisted.

- **M2: No mention of SIGTERM/SIGINT signal handling in `ao start`.** The ADR mentions "Ctrl-C" triggers shutdown (line 133, 147) and the design doc mentions SIGTERM (line 153), but neither specifies the Rust signal handling mechanism. For tokio, this is typically `tokio::signal::ctrl_c()` and a signal handler for SIGTERM. The `shutdown` watch channel is mentioned but the signal-to-channel bridge is not specified. **Recommendation:** Note that `tokio::signal::ctrl_c()` feeds the `shutdown` watch channel, and SIGTERM is handled via `tokio::signal::unix::signal(SignalKind::terminate())`.

- **M3: `ao batch-spawn` does not support `--agent` flag.** `ao spawn` supports `--agent <name>` to override the default agent, but `ao batch-spawn` (ADR line 169, design doc line 194-195) does not. The PRD does not explicitly specify this, but it is a reasonable expectation. A batch of issues spawned with a non-default agent requires multiple `ao spawn` calls. **Recommendation:** Add `--agent <name>` to `ao batch-spawn` or document the omission as intentional.

- **M4: Socket path may exceed Unix domain socket path length limit.** Unix domain socket paths are limited to 104 bytes on macOS (108 on Linux). `DataPaths.root` is `~/.agent-orchestrator/{sha256-12chars}-{projectId}/orchestrator.sock`. With a long home directory path and a long project ID, this could exceed the limit. Example: `/Users/geraldyeo/.agent-orchestrator/abcdef123456-my-very-long-project-name/orchestrator.sock` is 87 bytes — close to the limit. **Recommendation:** Document the path length constraint, or use a shorter socket name (e.g., `ao.sock`), or consider placing the socket in a shorter path like `/tmp/ao-{hash}.sock`.

- **M5: IPC protocol has no version field or backward compatibility mechanism.** The ADR acknowledges this in consequences (line 288) but defers versioning to post-MVP. However, the `OrchestratorRequest` and `OrchestratorResponse` enums use `#[serde(tag = "type")]` which means unknown variants will fail deserialization. If a newer CLI sends a request to an older orchestrator (or vice versa), it will get a deserialization error rather than a clean "unsupported request" response. **Recommendation:** Add a `version: u32` field to requests (set to 1 at MVP) so the orchestrator can reject incompatible clients with a clear error rather than a serde parse failure.

- **M6: `ao session cleanup` has an ambiguity about which sessions it targets.** The ADR (line 186-191) says it filters "non-terminal sessions" then checks tracker state. But the PRD (FR6 line 135) says "Kill sessions where PR is merged or issue is closed." The ADR only checks tracker state (issue closed), not PR merge status. A session in `pr_open` status whose PR was merged but whose issue is still open would not be caught by `ao session cleanup`. **Recommendation:** Clarify whether cleanup also checks PR merge status (requires SCM/GitHub API call) or is intentionally limited to tracker state only at MVP.

### Low

- **L1: `ao session ls` is described as "alias for `ao status`" (ADR line 171, design doc line 252-253) but their flags differ.** `ao status` has `[-p <project>] [--json]` while `ao session ls` shows `[-p <project>]` without `--json`. Since `--json` is a global flag, this is technically consistent, but the design doc could be clearer that `ao session ls` inherits all global flags. Minor documentation nit.

- **L2: The 500ms delay between batch spawns (ADR line 169, design doc line 202) is described as "PRD spec" but is not configurable.** If the delay proves too short (rate limiting) or too long (slow batch spawns), there is no escape hatch. **Recommendation:** Consider making this a constant with a config override path post-MVP, or at least document it as a tunable.

- **L3: The `ao send` fallback mode (without orchestrator) is described inconsistently.** ADR line 109 says fallback requires `--no-wait` to be specified. ADR line 293 says fallback happens when `--no-wait` is specified "or the orchestrator must be down." The design doc (line 226) says the fallback happens when the orchestrator is not running, regardless of `--no-wait`. **Recommendation:** Clarify: does fallback mode activate (a) only with `--no-wait`, (b) only when orchestrator is down, or (c) in either case? The design doc's version (b) seems most useful.

- **L4: `ao init` interactive mode is underspecified.** The design doc (line 129) mentions prompting for "project ID, repo, path, agent, runtime" but does not mention validation of interactive inputs or what happens on Ctrl-C mid-wizard. Minor — implementation detail rather than architecture.

- **L5: Exit code 0 for `ao stop` when already stopped (idempotent, ADR line 197) is good design. However, exit code 4 for "orchestrator not running" on other mutating commands could confuse scripts that check for both conditions.** A script running `ao stop && ao spawn ...` would succeed on `ao stop` (exit 0) but then fail on `ao spawn` (exit 4) if the orchestrator was never started. This is correct behavior but may be surprising. Minor — documenting the interaction would suffice.
