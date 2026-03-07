# ADR-0007 Review -- Round 2 (Gemini)

## Summary

All six High findings from round 1 have been addressed. The concurrency model is now explicitly channel-based (mpsc + oneshot), the `ao send` fallback is simplified to direct `tmux send-keys` via `CommandRunner`, `ao session kill` provides 5s polling feedback, deferred flags are documented, and the `ao spawn` positional change and `ao stop` global scope are justified. Medium and Low findings are also resolved or acceptably mitigated. No new Critical or High issues were introduced by the changes.

## Verdict

Accept

## Round 1 Finding Verification

### High Findings -- All Resolved

- **H1: Orchestrator concurrency model.** Resolved. ADR lines 141-151 and design doc lines 379-389 describe the mpsc channel approach with a clear ASCII diagram. The `Orchestrator` struct (ADR lines 136-137) now includes `ipc_tx` and `ipc_rx` fields. The guarantee is explicit: "no concurrent mutation of session state, no lock ordering issues." The design note that IPC requests are drained at the start of each tick (before the gather phase) is a sound sequencing choice.

- **H2: `ao send` fallback path.** Resolved. ADR line 123 and design doc line 240 specify the fallback as a direct `tmux send-keys` call via `CommandRunner` — no `Runtime` construction. The rationale is clear: tmux is the only MVP runtime, and `send-keys` only needs the session name from `SessionMetadata`. The warning message is documented.

- **H3: `ao session kill` UX feedback.** Resolved. ADR line 204 and design doc line 277 specify the CLI polls `SessionStore` up to 5s (every 500ms) after setting the flag. Two outcomes: "Session killed." (confirmed within 5s) or "Kill scheduled, will complete within {poll_interval}s." This provides good UX for the common case without blocking indefinitely.

- **H4: `--no-orchestrator` and `--rebuild` deferred.** Resolved. ADR lines 288-289 and design doc lines 418-419 add both to the deferred items table with clear rationale.

- **H5: `ao stop` is global.** Resolved. ADR line 261 documents the rationale: "global because the orchestrator is a single foreground process managing all projects. Per-project stop has no meaning in this model."

- **H6: `ao spawn` positional arg change.** Resolved. ADR line 263 justifies the change: "consistency with all other project-scoped commands and zero-friction single-project ergonomics."

### Medium Findings -- All Resolved

- **M1: `BatchSpawnItem` defined.** Resolved. ADR lines 89-101 define `BatchSpawnItem` and `BatchSpawnOutcome` with three variants (`Spawned`, `Skipped`, `Failed`). Design doc lines 90-102 match.

- **M2: Socket path clarified.** Resolved. ADR line 64 specifies `~/.agent-orchestrator/orchestrator.sock` with an explicit parenthetical: "(orchestrator-wide, not per-project — `DataPaths` is per-project but the orchestrator is a single process managing all projects)."

- **M3: `ao status` data access documented.** Resolved. ADR line 123 documents the full path: "loading config via `config::load()`, constructing `DataPaths` per project, and iterating `SessionStore::list()` — lightweight, no plugin construction."

- **M4: Duplicate detection matching clarified.** Resolved. ADR line 204 specifies: "non-terminal sessions whose `session_id` starts with `{prefix}-{issueId}-` (trailing hyphen prevents `myproj-4` matching `myproj-42`)."

- **M5: `Kill` request project resolution.** Partially resolved. The `Kill` request still takes only `session_id` without `project_id`. The orchestrator must iterate all project stores to find the session. At MVP scale (single-digit projects) this is trivial. Downgraded to Low -- see L6 below.

### Low Findings -- Resolved or Acceptable

- **L1: `ao init` interactive mode.** Acceptable. Design doc line 143 adds "Validates inputs inline." Prompt library choice is an implementation detail that does not need ADR-level specification.

- **L2: `--no-orchestrator`/`--rebuild` deferred.** Resolved (same as H4).

- **L3: Ctrl-U assumption documented.** Resolved. ADR line 319 documents the assumption and the post-MVP mitigation: `Agent::clear_input_step() -> Option<RuntimeStep>`.

- **L4: `ao session ls` alias vs. divergence.** Minor inconsistency remains. Design doc line 30 says "session focus" in the command tree, but line 267 says "Same output, same implementation. Exists for discoverability." Not blocking -- the divergence point (if any) is post-MVP.

- **L5: Signal handling documented.** Resolved. ADR line 153 documents SIGHUP as reserved for post-MVP config hot-reload and SIGTERM handled via `tokio::signal::unix::signal(SignalKind::terminate())`.

## New Findings

### Critical

(None)

### High

(None)

### Medium

(None)

### Low

- **L6: `Kill` request lacks `project_id` — orchestrator iterates all stores.** Carried from M5, downgraded. The `Kill` request (ADR line 73) takes only `session_id`. The orchestrator must scan all project stores to locate the session. At MVP scale this is a non-issue (HashMap iteration over single-digit entries). Post-MVP, if project count grows, consider either: (a) adding `project_id` to `Kill` for O(1) lookup, (b) maintaining a reverse index (`session_id -> project_id`) in the orchestrator, or (c) encoding the project in the session ID format. No action needed for MVP.

- **L7: `Orchestrator` struct holds both `ipc_tx` and `ipc_rx` but `run()` must move `ipc_rx` into the poll loop task.** The `mpsc::Receiver` is not `Clone` and must be moved into exactly one task. Holding it in the struct means `run()` must either consume `self` or use `Option<mpsc::Receiver>` with `.take()`. This is a minor implementation detail -- the design intent is clear. During implementation, consider creating the channel inside `run()` or using `Option` wrapping.

## Summary Assessment

The ADR is thorough, well-integrated with ADRs 0001-0006, and addresses all round 1 findings. The channel-based concurrency model is the right choice for serializing mutations. The IPC control plane cleanly separates read-only (direct file access) from mutating (orchestrator-coordinated) operations. The command structure is ergonomic for both human and script consumers. The two new Low findings are implementation-level details that do not affect the architecture.
