# ADR-0004: Plugin System, Agent Contract & Runtime

## Status
Accepted

## Context

The orchestrator's 8-slot plugin architecture (FR3), multi-agent support (FR1), and runtime execution model are deeply intertwined. The Agent produces commands; the Runtime executes them. The lifecycle engine (ADR-0001) coordinates both. These three concerns cannot be designed in isolation — the Agent trait's output is the Runtime trait's input, and the plugin framework governs how both are registered and instantiated.

Three prior ADRs constrain the design:

1. **ADR-0001 (Session Lifecycle Engine)** defines the poll loop with three phases: gather (I/O-heavy), evaluate (pure graph walk), transition (side effects). Agent and Runtime traits must serve the gather phase (activity detection, liveness checks) and spawn sequence (plan execution).
2. **ADR-0002 (Implementation Language)** locks in `Box<dyn Trait>` + `async-trait` for plugin dispatch, `tokio` for async, and Rust's exhaustive matching as a correctness tool.
3. **ADR-0003 (Configuration System)** settles `AgentConfig` (permissions, model, maxTurns, sandbox) and `#[serde(flatten)]` for forward-compatible agent-specific extras.

Key forces:

- The PRD specifies three prompt delivery modes (`inline`, `post-launch`, `protocol`) with fundamentally different launch sequences. The design must accommodate all three without hardcoding mode-specific logic in the lifecycle engine.
- Six activity states (`active`, `ready`, `idle`, `waiting_input`, `blocked`, `exited`) are inputs to the lifecycle engine's transition table. Detection varies by agent: JSONL log parsing (Claude Code), terminal output parsing (Codex, Aider), and protocol queries (OpenClaw).
- All plugins ship with the binary at MVP. Dynamic loading is a post-MVP concern.

## Considered Options

### Plugin Framework

1. **Static factory functions with `match` dispatch** — one factory function per slot (`create_agent()`, `create_runtime()`). Compiled-in `match` on plugin name. Pros: exhaustive, zero-allocation, trivially auditable. Cons: adding a plugin requires modifying the match and recompiling (acceptable for compiled-in plugins).

2. **Trait-based factory registry** — `HashMap<String, Box<dyn Factory>>` populated at startup. Pros: extensible at runtime. Cons: unnecessary indirection when all plugins are compiled in; loses exhaustive matching; more boilerplate.

### Agent-Runtime Interaction

3. **Declarative plans (LaunchPlan)** — Agent produces an ordered `Vec<RuntimeStep>` that the lifecycle engine feeds to `Runtime::execute_step()` one at a time. Agent and Runtime never reference each other. The `RuntimeStep` enum is the shared vocabulary. Pros: decoupled, testable (assert plan contents without I/O), new delivery modes are new plans (not new engine code), uniform execution loop. Cons: indirection between Agent intent and Runtime execution; Runtime must handle `UnsupportedStep` errors.

4. **Agent orchestrates launch (fat Agent)** — Agent trait takes a `&dyn Runtime` and handles its own launch. Pros: full per-agent control. Cons: tight coupling between slots; agents harder to test without Runtime mocks; lifecycle engine's role is diminished.

5. **Lifecycle engine hardcodes flow** — Agent returns data (command, delivery mode); lifecycle engine has a `match` on mode. Pros: simple. Cons: every new delivery mode modifies the engine; Agent loses agency over its launch sequence.

### Runtime Dispatch

6. **Single `execute_step(RuntimeStep)` method** — Runtime has one method that dispatches on step type internally. Pros: uniform plan execution; new step types extend the enum, not the trait. Cons: Runtime implementations need a `match` on all step variants.

7. **Individual methods per operation** — `create()`, `send_message()`, `send_protocol()` as separate trait methods. Pros: explicit interface. Cons: plan execution requires a `match` in the *engine*; adding a step type is a breaking trait change.

## Decision

**Plugin framework:** Option 1 — static factory functions. With ~3 plugins per slot at MVP and all compiled in, a `match` is sufficient and provides exhaustive compile-time checking.

**Agent-Runtime interaction:** Option 3 — declarative plans. The Agent produces a `LaunchPlan` containing `RuntimeStep` values. The lifecycle engine iterates and delegates to the Runtime. Neither trait references the other.

**Runtime dispatch:** Option 6 — single `execute_step()` method. The Runtime receives individual steps and dispatches internally. New step types are additive (extend enum + match arm), not breaking (no trait signature change).

**The core design has six components:**

### 1. Plugin Metadata and Registration

Each plugin carries a `PluginMeta { name, version, description }` via a `meta()` method on its slot trait. One factory function per slot uses `match` dispatch:

```rust
pub fn create_agent(name: &str, config: &AgentConfig) -> Result<Box<dyn Agent>, PluginError>;
pub fn create_runtime(name: &str, config: &Config) -> Result<Box<dyn Runtime>, PluginError>;
```

Startup validation checks all plugin names referenced in config are known, before any sessions are created.

### 2. RuntimeStep — The Shared Vocabulary

```rust
pub enum RuntimeStep {
    Create { command: Vec<String>, env: HashMap<String, String>, working_dir: PathBuf },
    WaitForReady { timeout: Duration },
    SendMessage { content: String },
    SendBuffer { content: String },
    SendProtocol { payload: Vec<u8> },
}
```

This enum is the decoupling seam. Agents produce steps without knowing if the Runtime is tmux or Docker. Runtimes execute steps without knowing if the Agent is Claude Code or Codex. Unsupported steps return `RuntimeError::UnsupportedStep`.

### 3. Runtime Trait

```rust
#[async_trait]
pub trait Runtime: Send + Sync {
    fn meta(&self) -> PluginMeta;
    async fn execute_step(&self, session_id: &str, step: &RuntimeStep) -> Result<(), RuntimeError>;
    async fn get_output(&self, session_id: &str, lines: usize) -> Result<String, RuntimeError>;
    async fn is_alive(&self, session_id: &str) -> Result<bool, RuntimeError>;
    async fn destroy(&self, session_id: &str) -> Result<(), RuntimeError>;
    fn attach_info(&self, session_id: &str) -> Option<String> { None }
    fn supported_steps(&self) -> &'static [&'static str] {
        &["create", "wait_for_ready", "send_message", "send_buffer", "send_protocol"]
    }
}
```

`supported_steps()` returns the step types this Runtime can execute. Used for plan validation at session creation — mismatches (e.g., `SendProtocol` on tmux) are caught before execution, not mid-plan. Default returns all steps; implementations override to exclude unsupported ones.

The tmux implementation maps steps to tmux commands: `Create` → `tmux new-session`, `SendMessage` → `tmux send-keys`, `SendBuffer` → `tmux load-buffer` + `paste-buffer`, `WaitForReady` → poll `tmux has-session`, `SendProtocol` → `UnsupportedStep`. All tmux commands go through `CommandRunner` (ADR-0002).

**`WaitForReady` semantics:** `WaitForReady` checks that the runtime session exists (e.g., `tmux has-session`), not that the agent is ready for input. "Session exists" and "agent ready" are distinct — the agent process may still be initializing after the session is created. For post-launch plans that follow `WaitForReady` with `SendMessage`, agents must tolerate early input delivery. Claude Code handles this (input is buffered). Agents that require readiness confirmation should use additional `WaitForReady` with output-pattern matching (deferred to post-MVP).

### 4. Agent Trait

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    fn meta(&self) -> PluginMeta;
    fn launch_plan(&self, ctx: &LaunchContext) -> LaunchPlan;
    fn detect_activity(&self, ctx: &GatherContext) -> ActivityState;
    fn parse_session_info(&self, ctx: &GatherContext) -> AgentSessionInfo;

    // Post-MVP, optional with defaults:
    fn continue_plan(&self, ctx: &ContinueContext) -> ContinuePlan { .. }
    fn restore_plan(&self, ctx: &RestoreContext) -> Option<RestorePlan> { None }
    fn shutdown_plan(&self, ctx: &ShutdownContext) -> ShutdownPlan { .. }
    fn workspace_hooks(&self, ctx: &WorkspaceHookContext) -> Vec<AgentWorkspaceHook> { vec![] }
}
```

Only `launch_plan`, `detect_activity`, and `parse_session_info` are required. The Agent reads `LaunchContext` (prompt, workspace path, session ID, agent config, environment) and returns a `LaunchPlan { steps: Vec<RuntimeStep> }`.

**`GatherContext` — making I/O explicit.** Both reviewers identified that `detect_activity()` and `parse_session_info()` performed hidden filesystem I/O for JSONL-based agents, breaking the gather-phase I/O model from ADR-0001. The fix: the lifecycle engine's gatherer is responsible for **all** I/O. It populates a `GatherContext` with terminal output (from `runtime.get_output()`) and agent-specific auxiliary data (e.g., JSONL log content read from disk). Agent methods are pure functions over this context:

```rust
pub struct GatherContext {
    pub terminal_output: String,            // from runtime.get_output()
    pub auxiliary_log: Option<String>,       // agent-specific: JSONL content, protocol response, etc.
    pub auxiliary_log_path: Option<PathBuf>, // path the gatherer read from (for diagnostics)
}
```

The gatherer knows which auxiliary path to read from a method on the Agent trait:

```rust
// On Agent trait:
fn auxiliary_log_path(&self) -> Option<PathBuf> { None }
```

Claude Code returns `Some(~/.claude/projects/.../logs/*.jsonl)`. Terminal-only agents return `None`. The gatherer reads the file and populates `GatherContext.auxiliary_log`. Agent detection methods remain pure and testable without mocks.

**`AgentSessionInfo`** (renamed from `SessionInfo` to clarify scope) is agent-extracted metadata, not the full session record. The lifecycle engine writes these values into the session metadata file alongside engine-sourced fields (`terminationReason`, action journal, etc.):

```rust
pub struct AgentSessionInfo {
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
}
```

`parse_session_info()` is called every poll tick so that token counts are fresh for budget evaluation (ADR-0001 `budgetExceeded` global edge).

### 5. Plan Type System

Each lifecycle phase has its own plan type:

- `LaunchPlan { steps: Vec<RuntimeStep> }` — MVP, required
- `ContinuePlan { steps: Vec<RuntimeStep> }` — post-MVP, for multi-turn nudges
- `RestorePlan { steps: Vec<RuntimeStep> }` — post-MVP, for crashed session recovery
- `ShutdownPlan { steps: Vec<ShutdownStep> }` — post-MVP, with phase-specific `ShutdownStep` enum containing `ForceKill` and `DestroyRuntime` (invalid in launch context)

Per-phase step enums prevent invalid plan construction at compile time. `LaunchPlan` cannot contain `ForceKill`; `ShutdownPlan` cannot contain `Create`.

**`ShutdownStep` execution path.** `ShutdownStep` is **not** executed via `Runtime::execute_step()` — that method only accepts `RuntimeStep`. Instead, the lifecycle engine maps `ShutdownStep` variants to direct Runtime method calls: `ShutdownStep::SendMessage` → `runtime.execute_step(RuntimeStep::SendMessage { .. })`, `ShutdownStep::WaitForExit` → poll `runtime.is_alive()` with timeout, `ShutdownStep::ForceKill` → `runtime.destroy()`, `ShutdownStep::DestroyRuntime` → `runtime.destroy()`. The engine interprets `ShutdownStep` rather than delegating it wholesale, because shutdown involves lifecycle-level decisions (when to escalate from graceful to force kill) that belong in the engine, not the Runtime.

### 6. Plan Execution Failure Semantics

When a step fails mid-plan, the lifecycle engine:

1. **Aborts the plan** — remaining steps are not executed.
2. **Cleans up** — if a `Create` step succeeded earlier in the plan, the engine calls `runtime.destroy(session_id)` to reclaim the partially-created session.
3. **Transitions to `errored`** — the session status is set to `errored` with the failing step index and error in session metadata.
4. **Logs structured context** — each step is logged as a structured event before execution (step index, step type, parameters) and after (success/failure, duration, error detail). On failure, the full plan is logged with the failing step highlighted.

Common failure modes:
- `WaitForReady` timeout (agent process failed to start) → destroy runtime, set `errored`.
- `SendMessage` after `Create` (runtime created but message delivery failed) → destroy runtime, set `errored`.
- `UnsupportedStep` (plan/runtime mismatch) → destroy runtime, set `errored`. Mitigated by plan validation via `supported_steps()` at session creation.

### 7. Activity State Detection

`detect_activity()` returns one of six `ActivityState` variants. Detection is agent-specific: Claude Code parses JSONL logs (with terminal fallback), terminal-based agents use regex patterns, protocol-based agents query session state.

`Idle` is not detected by the Agent. The Agent reports `Ready`; the lifecycle engine's gatherer promotes `Ready` to `Idle` based on elapsed time past `readyThresholdMs`. This is consistent with ADR-0001's decision that timer-based triggers are handled in the gather phase.

### PRD Interface Mapping

**FR1 Agent methods → ADR trait methods:**

| PRD Method | ADR Mapping |
|------------|-------------|
| `getLaunchCommand()` | `launch_plan()` (subsumes — returns full step sequence, not just a command) |
| `getEnvironment()` | `launch_plan()` via `LaunchContext.env_extras` (environment is part of `RuntimeStep::Create`) |
| `getActivityState()` | `detect_activity()` |
| `isProcessRunning()` | Moved to `Runtime::is_alive()` — process liveness is a Runtime concern, not Agent |
| `getSessionInfo()` | `parse_session_info()` (renamed to `AgentSessionInfo` to clarify scope) |
| `getRestoreCommand()` | `restore_plan()` (returns full step sequence, not just a command) |
| `postLaunchSetup()` | Subsumed by multi-step `LaunchPlan` — post-launch setup is additional steps after `Create` |
| `setupWorkspaceHooks()` | `workspace_hooks()` |

**FR3 Runtime methods → ADR trait methods:**

| PRD Method | ADR Mapping |
|------------|-------------|
| `create()` | `execute_step(RuntimeStep::Create { .. })` |
| `destroy()` | `destroy()` (direct method — not a step, since it tears down the execution context) |
| `sendMessage()` | `execute_step(RuntimeStep::SendMessage { .. })` and `execute_step(RuntimeStep::SendBuffer { .. })` — split into keystroke vs. buffer delivery |
| `getOutput()` | `get_output()` |
| `isAlive()` | `is_alive()` |
| `getMetrics()` | Not on Runtime. Token tracking is agent-sourced via `parse_session_info()`, not runtime-sourced. The PRD lists `getMetrics()` as optional; this ADR defers it because the primary token data source is agent JSONL logs. |
| `getAttachInfo()` | `attach_info()` |

### `AgentWorkspaceHook` Type

Agent-specific hooks are distinct from the four config-driven workspace hooks (`afterCreate`, `beforeRun`, `afterRun`, `beforeRemove`) defined in FR2. Config-driven hooks are shell commands specified in YAML; agent hooks are programmatic callbacks registered by the agent plugin.

```rust
pub enum AgentWorkspaceHook {
    /// Script injected as a Claude Code PostToolUse hook.
    PostToolUse { script: String },
    /// Script run after workspace creation (agent-specific setup).
    AfterCreate { script: String },
}
```

The lifecycle engine merges agent hooks with config hooks at workspace creation time. Agent hooks run after config hooks to allow agent-specific overrides.

### `prompt_delivery` Configuration

The PRD (FR1) specifies `promptDelivery` as a per-agent property. This is added to `AgentConfig` as a typed field (not relegated to `extra`), since it is a cross-agent concern:

```rust
// In AgentConfig (ADR-0003 schema.rs):
pub prompt_delivery: Option<PromptDelivery>,  // None = agent's default

pub enum PromptDelivery {
    Inline,      // prompt as CLI arg (default for most agents)
    PostLaunch,  // prompt sent via runtime.sendMessage() after agent starts
    Protocol,    // prompt sent via JSON-RPC / ACP
}
```

The Agent reads `ctx.agent_config.prompt_delivery` in `launch_plan()` to decide which step sequence to produce.

### Session ID Format

Session IDs follow the format `{sessionPrefix}-{issueId}-{attempt}`:
- `sessionPrefix`: from config (auto-derived from project ID if not specified), validated tmux-safe
- `issueId`: tracker issue identifier (e.g., `42`, `ABC-123`)
- `attempt`: monotonically increasing integer per issue, starting at 1

Example: `myproject-42-1`, `myproject-42-2` (retry).

The lifecycle engine guarantees uniqueness: it checks for existing sessions with the same prefix-issueId before spawning and increments the attempt counter. tmux session names are validated against `[a-zA-Z0-9._-]`.

### Prompt Delivery Modes

All three PRD delivery modes (`inline`, `post-launch`, `protocol`) are expressed as different `LaunchPlan` step sequences produced by the Agent, not as engine-level `match` branches:

- **Inline:** `[Create { command: ["claude", "-p", prompt] }]`
- **Post-launch:** `[Create { command: ["claude"] }, WaitForReady { 10s }, SendMessage { prompt }]`
- **Protocol:** `[Create { command: ["acpx", "serve"] }, WaitForReady { 10s }, SendProtocol { json_rpc }]`

The lifecycle engine executes all three identically: iterate steps, call `runtime.execute_step()`.

### MVP Implementations

- **`claude-code` agent:** JSONL-based activity detection, inline and post-launch plans, restore via `--continue`, PostToolUse workspace hook.
- **`tmux` runtime:** Full `RuntimeStep` support except `SendProtocol`.
- **All other agents:** Stubs that return `PluginError::NotImplemented` from the factory.

Reference `docs/plans/2026-03-06-plugin-system-agent-runtime-design.md` for full pseudocode, tmux command mapping, testing strategy, and module structure.

## Consequences

Positive:

- Agent and Runtime are fully decoupled — neither trait references the other. The `RuntimeStep` enum is the only shared type. This means new agents and new runtimes can be developed independently.
- Plans are testable without I/O — construct a `LaunchContext`, call `launch_plan()`, assert the returned steps. No mocks, no runtime, no tmux.
- New prompt delivery modes require only a new plan sequence in the Agent, not changes to the lifecycle engine. The `protocol` mode (OpenClaw via ACP) works by producing `SendProtocol` steps — no special engine handling.
- The `execute_step()` pattern means new step types are additive (extend enum + add match arms in implementations), not breaking trait changes.
- The plugin framework is minimal by design — static factory functions are sufficient for compiled-in plugins and can be replaced by a registry if dynamic loading is needed post-MVP.
- The plan pattern extends naturally to other lifecycle phases (`ContinuePlan`, `RestorePlan`, `ShutdownPlan`) via optional trait methods with defaults, enabling incremental delivery.

Negative:

- Indirection between Agent intent and Runtime execution. When debugging "why didn't the agent launch?", you need to inspect both the plan (what steps were produced) and the execution (how the Runtime handled each step). Mitigated by logging each step before and after execution.
- `RuntimeStep` is a shared enum that both Agent and Runtime depend on. Adding a new step type requires updating all Runtime implementations (to handle or reject it). With 2 runtimes at MVP this is manageable; with many runtimes post-MVP, a `supports_step()` capability declaration may be needed.
- Stubs for 5 of 6 agents means the Agent factory `match` has many arms that return `NotImplemented`. Minor boilerplate, but signals to reviewers that the plugin system is designed for breadth even though MVP only implements depth on `claude-code`.
- `GatherContext` requires the lifecycle engine's gatherer to know about agent-specific auxiliary log paths. The `auxiliary_log_path()` method on Agent pushes this knowledge into the trait, but the gatherer must read potentially large log files every tick. Mitigated by reading only the tail of the log (last N lines, similar to `runtime.get_output()`).
- `ShutdownPlan` uses a separate `ShutdownStep` enum executed via engine-level mapping (not `execute_step()`), introducing asymmetry with `LaunchPlan`'s uniform `execute_step()` flow. This is intentional — shutdown involves lifecycle-level escalation decisions — but adds conceptual overhead.
- `RuntimeStep` enum should be marked `#[non_exhaustive]` post-MVP to allow additive changes without breaking downstream Runtime implementations. At MVP with 1-2 runtimes, exhaustive matching is preferred for compiler-enforced completeness.
- Claude Code's `auxiliary_log_path` depends on Claude Code's internal log file location (`~/.claude/projects/.../logs/*.jsonl`), which may change without notice. This is an implicit contract with an external tool — a maintenance risk acknowledged but accepted since JSONL detection is significantly more reliable than terminal parsing.
