# ADR-0007 Review -- Round 1 (Gemini)

## Summary

ADR-0007 is a well-structured and thorough design for the CLI layer. It makes sound architectural choices -- foreground process model, Unix domain socket IPC, orchestrator-as-coordinator for mutating operations, and direct file access for reads. The integration points with ADRs 0001-0006 are mapped explicitly and are largely consistent. The main concerns are around concurrency safety in the Orchestrator struct, a gap in the `ao send` fallback path, and a few places where the ADR introduces behavior not fully grounded in prior ADRs.

## Verdict

Accept with conditions

## Strengths

- **IPC control plane is well-motivated.** The decision to route all mutating operations through the orchestrator via Unix domain socket eliminates race conditions between concurrent CLI invocations and tmux sessions. The rationale against HTTP and gRPC is convincing. The socket-as-liveness-check is elegant.
- **Command routing table is clear and auditable.** The IPC/no-IPC split is well-reasoned -- read-only commands bypass IPC for resilience, mutating commands serialize through the orchestrator. The table in the ADR (lines 96-108) and the design doc (lines 106-117) match.
- **PRD coverage is thorough.** All 13 FR6 commands are accounted for, with a clear MVP/deferred mapping table. The deferred items table (lines 254-268) provides explicit rationale for each deferral, and the post-MVP priorities are sensible (orchestrator-as-session as highest priority).
- **Error handling is production-quality.** The 5-tier exit code system, human vs JSON error formatting, stdout/stderr separation, and progress-to-stderr convention are all well thought out. The `ao stop` idempotency (exit 0 when already stopped) is a nice touch.
- **Project auto-resolution is pragmatic.** The 4-step resolution algorithm (explicit flag, single project, CWD match, error) handles the common case (single project) with zero friction while remaining unambiguous for multi-project setups.
- **Design doc testing strategy** covers unit, integration, and manual testing with specific scenarios for each command. The IPC round-trip and stale socket tests are particularly valuable.

## Findings

### Critical

(None)

### High

- **H1: `Orchestrator` struct lacks interior mutability for concurrent access.**
  The `Orchestrator` struct (ADR line 116, design doc line 355) holds `stores: HashMap<String, SessionStore>` and `plugins: HashMap<String, ProjectPlugins>`. The `run()` method spawns two concurrent tasks -- the poll loop and the IPC listener -- both of which need mutable access to session state (e.g., `handle_spawn` creates sessions, `handle_kill` sets `manualKill`). The struct takes `&self` on `run()`, but all handler methods also take `&self`. With two concurrent tasks mutating shared state, this needs `Arc<Mutex<...>>` or `Arc<RwLock<...>>` on the mutable fields, or a channel-based approach where IPC requests are sent to the poll loop task for serialized execution.
  **Recommendation:** Specify the concurrency model explicitly. The simplest correct approach is to have the IPC listener send requests via an `mpsc` channel to the poll loop task, which processes them between ticks. This serializes all mutations without locks and avoids priority inversion. Document this in the Orchestrator struct section.

- **H2: `ao send` fallback constructs Agent + Runtime without the orchestrator.**
  ADR line 109 says `ao send --no-wait` falls back to direct `Runtime::execute_step(SendMessage)` when the orchestrator is not running. But the CLI process does not hold Agent or Runtime instances -- those live in the Orchestrator (line 119, `plugins: HashMap<String, ProjectPlugins>`). The fallback path would need to: load config, create plugins (including `create_runtime()`), look up the session's runtime type from `SessionStore`, construct the runtime, execute the step, and tear down. This is significant -- it's not just "call `execute_step`."
  **Recommendation:** Either (a) document the full fallback construction sequence (load config, create runtime from session metadata, execute, drop), or (b) simplify the fallback to only work with tmux directly (since tmux sessions persist and `tmux send-keys` works without orchestrator state), or (c) remove the fallback entirely and require the orchestrator for `ao send`. Option (b) is pragmatic since tmux is the only MVP runtime.

- **H3: `ao session kill` delay is underspecified for UX.**
  ADR line 184 states the kill delay is "at most one poll interval (30s)." The CLI prints a message but does not wait for confirmation that the kill was processed. If the user runs `ao session kill` then immediately runs `ao status`, the session still shows as `working` (or whatever its pre-kill state was). This creates a confusing UX where the user thinks the kill failed.
  **Recommendation:** Add an option for the IPC response to include the current status and expected transition time. Alternatively, have the CLI poll `SessionStore` briefly (e.g., up to 5s) after the kill request to confirm the transition, with a fallback message "Kill scheduled, will complete within {poll_interval}s." This is a UX polish item, but given that `kill` is a high-intent action, the feedback matters.

### Medium

- **M1: `BatchSpawnItem` type is referenced but never defined.**
  ADR line 83 references `Vec<BatchSpawnItem>` in `BatchSpawnResult`, but the `BatchSpawnItem` struct is never defined in the ADR or design doc. The design doc (line 203) describes per-issue results as "spawned, skipped, or failed with reason" but does not provide the struct.
  **Recommendation:** Define `BatchSpawnItem` in the IPC types section:
  ```rust
  pub struct BatchSpawnItem {
      pub issue_id: String,
      pub result: BatchSpawnOutcome, // Spawned { session_id, branch }, Skipped { reason }, Failed { error }
  }
  ```

- **M2: Socket path uses `DataPaths.root` but `DataPaths` is per-project.**
  ADR line 64 says the socket lives at `{DataPaths.root}/orchestrator.sock`. But `DataPaths` is per-project (ADR-0005 line 67: `root = ~/.agent-orchestrator/{hash}-{projectId}/`). A multi-project config would have multiple `DataPaths` instances. Which project's root hosts the socket?
  **Recommendation:** The socket should live at the orchestrator-wide root (`~/.agent-orchestrator/orchestrator.sock`) or be derived from the config file path hash (not a project hash). Clarify that there is one socket per orchestrator instance, not per project. If the intent is `~/.agent-orchestrator/`, state this explicitly rather than referencing `DataPaths.root`.

- **M3: `ao status` reads `SessionStore` directly but `SessionStore` is per-project.**
  ADR line 101 says `ao status` reads `SessionStore` files directly. With `stores: HashMap<String, SessionStore>` (one per project), `ao status` without `-p` must iterate all projects' stores. The design doc (line 234) does not describe how `ao status` discovers which projects exist without loading the config and constructing `DataPaths` per project.
  **Recommendation:** Document that `ao status` (and `ao session ls`) loads the config via `config::load()`, constructs `DataPaths` per project, creates `SessionStore` instances, and aggregates results. This is lightweight (no plugin construction) but should be explicit.

- **M4: `ao spawn` duplicate detection uses `{prefix}-{issueId}` but ADR-0004 defines session ID as `{prefix}-{issueId}-{attempt}`.**
  ADR line 190 says the orchestrator checks for "an existing non-terminal session with the same `{prefix}-{issueId}`." This is a prefix match, not an exact match. The logic is correct (checking if any attempt for that issue is active), but the implementation detail matters: `SessionStore::list()` returns all sessions, and the check must filter by prefix. This should reference the session ID format from ADR-0004 (line 248) and clarify the matching strategy.
  **Recommendation:** State explicitly: "filter `SessionStore::list()` results where `session_id` starts with `{prefix}-{issueId}-`" (note the trailing hyphen to avoid false matches like `myproj-4` matching `myproj-42`).

- **M5: `Cleanup` request includes `project_id` but `Kill` request uses `session_id` -- project resolution asymmetry.**
  In the IPC request enum (ADR line 69-76), `Kill` takes `session_id` while `Cleanup` takes `project_id`. For `Kill`, the orchestrator must determine which project the session belongs to (to look up the correct `SessionStore`). The session ID format `{prefix}-{issueId}-{attempt}` uses the project's `sessionPrefix`, but two projects could theoretically use the same prefix.
  **Recommendation:** Either (a) add `project_id` to the `Kill` request for consistency with `Cleanup`, or (b) document that the orchestrator iterates all project stores to find the session (acceptable at MVP scale), or (c) enforce unique `sessionPrefix` across projects in config validation (ADR-0003).

### Low

- **L1: `ao init` interactive mode is underspecified.**
  The design doc (line 129) mentions prompting for "project ID, repo, path, agent, runtime" but does not describe validation during prompts (e.g., does it check `git remote get-url origin` interactively?), error recovery (e.g., invalid repo format), or the prompt library/approach for Rust (e.g., `dialoguer`, `inquire`).
  **Recommendation:** Mention the prompt library choice (or defer it) and note that interactive mode validates each input before proceeding to the next prompt.

- **L2: `ao start --no-dashboard` is accepted as a no-op but `ao start` in the PRD takes `[project|url]` as a positional arg.**
  The PRD (line 127) specifies `ao start [project|url]` with `--no-orchestrator` and `--rebuild` flags. ADR-0007 drops the positional argument entirely, which is correct for MVP, but does not mention `--no-orchestrator` or `--rebuild` in the deferred items table. These are minor but should be listed for completeness.
  **Recommendation:** Add `--no-orchestrator` and `--rebuild` flags to the deferred items table with rationale.

- **L3: The `\x15` (Ctrl-U) clear-input step in `ao send` assumes a Unix terminal line discipline.**
  ADR line 179 and design doc line 219 specify sending Ctrl-U to clear partial input before message delivery. This works for readline-based agents but may not work for agents with custom input handling (e.g., a TUI-based agent). At MVP with Claude Code + tmux this is fine, but the assumption should be documented.
  **Recommendation:** Add a note that the clear-input step is agent-specific behavior that may need to be part of the Agent trait post-MVP (e.g., `Agent::clear_input_step() -> Option<RuntimeStep>`).

- **L4: Design doc lists `ao session ls` as "alias for `ao status`" but the ADR says "alias for `ao status` with session focus."**
  Design doc line 253 says "Same output, same implementation." If it truly is an alias, the "session focus" qualifier in the command tree (line 29) is misleading. Clarify whether `ao session ls` will diverge from `ao status` post-MVP (e.g., showing only session-level detail without project-level summary).
  **Recommendation:** Pick one: either it is a strict alias (remove "session focus") or document the intended post-MVP divergence.

- **L5: No mention of signal handling beyond SIGTERM/Ctrl-C.**
  The design doc (line 153) mentions Ctrl-C and SIGTERM for graceful shutdown but does not mention SIGHUP (commonly used for config reload, which is deferred but should be reserved) or SIGQUIT (common for debug dumps). At minimum, SIGHUP should be documented as reserved for future hot-reload.
  **Recommendation:** Add a note that SIGHUP is reserved for post-MVP config hot-reload (ADR-0003 deferral) and is currently ignored (not used for shutdown).
