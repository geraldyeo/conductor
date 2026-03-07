# ADR-0007 Review -- Round 2 (Codex)

## Summary

All six High findings from round 1 have been addressed. The channel-based concurrency model (H1) is well-specified with a clear diagram. The `ao send` fallback (H2) is simplified to direct `tmux send-keys`. The `ao session kill` polling UX (H3) is practical. Deferred items (H4), `ao stop` global rationale (H5), and `ao spawn` positional-to-auto-resolve justification (H6) are all explicitly documented. Medium findings M1-M4 are resolved or deferred with rationale. No new Critical or High issues were introduced.

## Verdict

**Accept**

## Round 1 Finding Verification

### High Findings -- All Resolved

| ID | Finding | Status | How Addressed |
|----|---------|--------|---------------|
| H1 | Orchestrator concurrency model unspecified | Resolved | mpsc channel from IPC listener to poll loop, oneshot for responses. Diagram added (ADR lines 141-151). No locks. |
| H2 | `ao send` fallback required Runtime construction | Resolved | Fallback simplified to direct `tmux send-keys` via `CommandRunner` (ADR line 123). No Runtime/Agent construction. |
| H3 | `ao session kill` had no UX feedback | Resolved | CLI polls SessionStore for 5s (500ms intervals) after setting flag. Prints immediate confirmation or "will complete within {poll_interval}s" (ADR line 204). |
| H4 | `--no-orchestrator` and `--rebuild` missing | Resolved | Both added to Deferred Items table with rationale (ADR lines 288-289). |
| H5 | `ao stop` global behavior undocumented | Resolved | PRD Interface Mapping explicitly documents rationale: "ao stop is global because the orchestrator is a single foreground process managing all projects" (ADR line 261). |
| H6 | `ao spawn` positional arg change unjustified | Resolved | Rationale added: "consistency with all other project-scoped commands and zero-friction single-project ergonomics" (ADR line 263). |

### Medium Findings

| ID | Finding | Status | Notes |
|----|---------|--------|-------|
| M1 | `ao status` missing PRD columns | Deferred | Explicitly added to Deferred Items with rationale: "Requires persisting PollContext fields beyond SessionMetadata" (ADR line 290). Acceptable at MVP. |
| M2 | Signal handling unspecified | Resolved | `tokio::signal::ctrl_c()` and `SignalKind::terminate()` now specified (ADR line 153). SIGHUP reserved for hot-reload. |
| M3 | `--agent` missing from `ao batch-spawn` | Resolved | Added to command signature (ADR line 189) and `BatchSpawn` IPC variant (ADR line 71). |
| M4 | Socket path length risk | Resolved | Socket path moved to orchestrator-wide root `~/.agent-orchestrator/orchestrator.sock` (ADR line 64), significantly shorter than the per-project path. |
| M5 | IPC protocol version field | Open (acceptable) | Still deferred to post-MVP (ADR line 312). Risk is low at MVP with a single binary. |
| M6 | `ao session cleanup` does not check PR merge | Open (acceptable) | Cleanup still limited to tracker state. Checking PR merge would require SCM API calls, adding complexity. Tracker-only check is sufficient for MVP -- merged PRs typically lead to closed issues. |

### Low Findings

| ID | Finding | Status | Notes |
|----|---------|--------|-------|
| L1 | `ao session ls` flag discrepancy | Open | `--json` is global so it applies. Documentation nit only. |
| L2 | 500ms batch delay not configurable | Open | Acceptable as a constant at MVP. |
| L3 | `ao send` fallback inconsistency | Resolved | ADR line 123 and design doc line 240 are now consistent: fallback activates when orchestrator is not running. `--no-wait` only skips activity polling. |
| L4 | `ao init` interactive underspecified | Open | Implementation detail. |
| L5 | Exit code 0/4 interaction | Open | Correct behavior; documentation nit. |

## New Observations (Round 2)

### Low

- **L6: `Orchestrator` struct owns both `ipc_tx` and `ipc_rx`.** The `mpsc::Receiver` is not `Clone` and must be moved into exactly one task. Since `run()` needs to hand `ipc_tx` to the IPC listener and `ipc_rx` to the poll loop, the struct either needs to consume `self` in `run()` or wrap these in `Option` for `take()`. This is a minor implementation detail, not an architectural concern -- just worth noting during implementation.

- **L7: IPC request latency up to 30s for `ao spawn`.** The consequences section (ADR line 318) acknowledges this and suggests mid-tick channel checks as a mitigation. This is acceptable at MVP but should be prioritized if user feedback indicates spawn latency is a pain point. An alternative: the poll loop could use `tokio::select!` with both a timer and the mpsc receiver, processing IPC requests immediately rather than only between ticks.

## Cross-ADR Consistency

The ADR correctly references and integrates:
- ADR-0001: poll loop, crash recovery, `manualKill` global edge at precedence 0
- ADR-0002: Rust, clap, tokio, CommandRunner
- ADR-0003: `load()`, `discover_config_path()`, `generate_default()`
- ADR-0004: factory functions, `LaunchPlan`, `detect_activity()`, `RuntimeStep::SendMessage/SendBuffer`
- ADR-0005: `SessionStore` CRUD, `DataPaths::ensure_dirs()`, `.origin` collision detection, 10-step creation sequence
- ADR-0006: `create_tracker()` fail-fast, `get_issue()` + `classify_state()`, `branch_name()`, `add_comment()`

The `BatchSpawnItem` and `BatchSpawnOutcome` types are well-defined. The IPC protocol enums are complete for all MVP commands. The PRD Interface Mapping table covers all 13 PRD commands with explicit MVP/deferred status.

## Final Assessment

ADR-0007 is a thorough and well-integrated design. The channel-based concurrency model is clean and avoids the complexity of shared mutable state. The IPC control plane establishes a solid architectural foundation for the end-state supervisor model. The command surface is pragmatic -- 10 MVP commands covering the core workflow without over-committing to undelivered FRs. All round 1 High findings are resolved satisfactorily. No new blocking issues.
