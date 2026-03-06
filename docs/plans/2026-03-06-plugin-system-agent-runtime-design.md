# Plugin System, Agent Contract & Runtime Design

**Date:** 2026-03-06
**FR:** FR3 (Plugin Architecture), FR1 (Multi-Agent Support), Runtime (implicit)
**ADR:** 0004
**Status:** Draft
**Depends on:** ADR-0001 (Session Lifecycle Engine), ADR-0002 (Implementation Language), ADR-0003 (Configuration System)

## Overview

This design covers the full "launch and manage an agent" vertical: the generic plugin framework, the Agent trait (how to launch, detect activity, interpret output), and the Runtime trait (how to execute processes, send input, capture output). These three concerns are co-dependent and designed together.

The central design challenge is the Agent-Runtime interaction. An Agent knows *what* to run; a Runtime knows *how* to run it. The lifecycle engine (ADR-0001) decides *when*. This design introduces **declarative plans** — the Agent produces a sequence of typed steps, the lifecycle engine feeds them to the Runtime — so Agent and Runtime never reference each other.

## Approach

**Approach A + Declarative Plans** — thin, independent traits for Agent and Runtime. The Agent produces a `LaunchPlan` (ordered list of `RuntimeStep` values) that the lifecycle engine executes by calling `runtime.execute_step()` for each step. Neither trait knows about the other; the `RuntimeStep` enum is the shared vocabulary.

### Alternatives Considered

1. **Agent Orchestrates Launch (Fat Agent)** — Agent trait takes a Runtime reference and handles its own launch sequence (`agent.launch(&runtime, &workspace, prompt)`). Gives each agent full control over launch flow. Rejected: creates tight coupling between Agent and Runtime traits, makes agents harder to test in isolation, and the lifecycle engine (ADR-0001) already exists as the natural coordinator.

2. **Session Facade** — A `Session` struct composes Agent + Runtime + Workspace and provides high-level operations (`session.start(prompt)`, `session.send(msg)`). Rejected: adds an abstraction layer redundant with the lifecycle engine. Cross-cutting concerns (budget checks, action journal, notifications) would need to pierce the facade, defeating its purpose.

3. **Lifecycle Mediates with Hardcoded Flow (Vanilla Thin Traits)** — Agent returns command + prompt delivery mode as data; lifecycle engine has a `match` on delivery mode to decide the launch sequence. Rejected: every new delivery mode requires modifying the lifecycle engine. The plan-based approach moves this variation into the Agent, where it belongs.

## 1. Plugin Framework

ADR-0002 decided `Box<dyn Trait>` + `async-trait` for all slot traits. All plugins ship with the binary. What remains is registration, metadata, and validation.

### Plugin Metadata

Each plugin carries a static descriptor:

```rust
pub struct PluginMeta {
    pub name: &'static str,        // "claude-code", "tmux"
    pub version: &'static str,     // "0.1.0"
    pub description: &'static str, // "Claude Code AI agent"
}
```

Exposed via a `meta()` method on every slot trait. Used for `ao status`, logging, and debugging.

### Plugin Instantiation

One factory function per slot with compiled-in `match` dispatch:

```rust
pub fn create_agent(name: &str, config: &AgentConfig) -> Result<Box<dyn Agent>, PluginError> {
    match name {
        "claude-code" => Ok(Box::new(ClaudeCodeAgent::new(config)?)),
        "codex"       => Ok(Box::new(CodexAgent::new(config)?)),
        "aider"       => Ok(Box::new(AiderAgent::new(config)?)),
        "opencode"    => Ok(Box::new(OpenCodeAgent::new(config)?)),
        "gemini"      => Ok(Box::new(GeminiAgent::new(config)?)),
        "openclaw"    => Ok(Box::new(OpenClawAgent::new(config)?)),
        _ => Err(PluginError::UnknownPlugin { slot: "agent", name: name.into() }),
    }
}

pub fn create_runtime(name: &str, config: &Config) -> Result<Box<dyn Runtime>, PluginError> {
    match name {
        "tmux"    => Ok(Box::new(TmuxRuntime::new(config)?)),
        "process" => Err(PluginError::NotImplemented { slot: "runtime", name: name.into() }),
        _ => Err(PluginError::UnknownPlugin { slot: "runtime", name: name.into() }),
    }
}

// Similar for create_workspace(), create_tracker(), create_scm()
```

**Why not a trait-based factory registry?** With ~3 plugins per slot at MVP and all compiled in, a `HashMap<String, Box<dyn Factory>>` is over-engineering. A `match` is exhaustive, zero-allocation, and trivially auditable. If third-party plugins become a requirement post-MVP, a registry can be added without changing slot traits.

### Startup Validation

Runs once before any sessions are created. Checks that all plugin names referenced in config are known:

```rust
pub fn validate_plugin_config(config: &Config) -> Result<(), PluginError> {
    // Default plugins exist
    validate_plugin_name("runtime", &config.defaults.runtime)?;
    validate_plugin_name("agent", &config.defaults.agent)?;
    validate_plugin_name("workspace", &config.defaults.workspace)?;

    // Per-project overrides exist
    for (id, project) in &config.projects {
        if let Some(ref rt) = project.runtime {
            validate_plugin_name("runtime", rt)?;
        }
        if let Some(ref ag) = project.agent {
            validate_plugin_name("agent", ag)?;
        }
        if let Some(ref ws) = project.workspace {
            validate_plugin_name("workspace", ws)?;
        }
        if let Some(ref tr) = project.tracker.as_ref().map(|t| &t.plugin) {
            validate_plugin_name("tracker", tr)?;
        }
    }
    Ok(())
}
```

### Module Structure

```
packages/core/src/plugin/
├── mod.rs          // PluginMeta, PluginError, validate_plugin_config()
├── registry.rs     // create_agent(), create_runtime(), create_workspace(), ...
```

### Deferred

| Feature | When | Why |
|---------|------|-----|
| Dynamic plugin loading (shared libs / WASM) | Post-MVP | No third-party plugins yet |
| Plugin dependency resolution | Post-MVP | No cross-plugin dependencies at MVP |
| Plugin lifecycle hooks (init/shutdown) | Post-MVP | Constructors + `Drop` sufficient |

## 2. Runtime Trait

The Runtime is the execution environment. It knows how to run a process, send it input, and capture its output. It does **not** know what an "agent" is.

### Trait Signature

```rust
#[async_trait]
pub trait Runtime: Send + Sync {
    fn meta(&self) -> PluginMeta;

    /// Execute a single plan step. Returns when the step completes.
    async fn execute_step(
        &self,
        session_id: &str,
        step: &RuntimeStep,
    ) -> Result<(), RuntimeError>;

    /// Capture recent output from the session.
    async fn get_output(
        &self,
        session_id: &str,
        lines: usize,
    ) -> Result<String, RuntimeError>;

    /// Is the session process still running?
    async fn is_alive(&self, session_id: &str) -> Result<bool, RuntimeError>;

    /// Clean up the session (kill process, remove runtime resources).
    async fn destroy(&self, session_id: &str) -> Result<(), RuntimeError>;

    /// Info for human attach (e.g., "tmux attach -t foo"). Optional.
    fn attach_info(&self, session_id: &str) -> Option<String> {
        None
    }

    /// Step types this runtime supports. Used for plan validation at spawn.
    fn supported_steps(&self) -> &'static [&'static str] {
        &["create", "wait_for_ready", "send_message", "send_buffer", "send_protocol"]
    }
}
```

### RuntimeStep Enum

The shared vocabulary between Agent plans and Runtime execution:

```rust
pub enum RuntimeStep {
    /// Launch a process in the runtime.
    Create {
        command: Vec<String>,
        env: HashMap<String, String>,
        working_dir: PathBuf,
    },
    /// Wait until the session is alive and responsive.
    WaitForReady {
        timeout: Duration,
    },
    /// Send text input to the session (keystrokes).
    SendMessage {
        content: String,
    },
    /// Send text via a file buffer (for large payloads that exceed keystroke limits).
    SendBuffer {
        content: String,
    },
    /// Send a structured protocol message (JSON-RPC, ACP).
    SendProtocol {
        payload: Vec<u8>,
    },
}
```

### Why `execute_step()` Instead of Individual Methods

An earlier draft had separate `create()`, `send_message()`, `send_protocol()` methods. The step-based approach is better because:

1. **Plans execute uniformly** — the lifecycle engine iterates over steps with one call per step, no `match` on step type at the engine level.
2. **New step types** only require extending the `RuntimeStep` enum and adding a match arm in implementations, not changing the trait signature.
3. **Logging and journaling** are natural — each step is a discrete, loggable event with structured data.

The trade-off: Runtime implementations have a `match` inside `execute_step()`. This is the right place for it — the Runtime knows which steps it supports.

### Unsupported Steps

Not every Runtime supports every step type. The `process` runtime cannot do `SendBuffer` (no tmux buffer). The contract:

```rust
// Inside execute_step()
RuntimeStep::SendBuffer { .. } => {
    Err(RuntimeError::UnsupportedStep {
        step: "send_buffer",
        runtime: self.meta().name,
    })
}
```

Plan validation runs at session creation via `runtime.supported_steps()` to catch mismatches early: "this agent's launch plan requires `SendProtocol` but the `tmux` runtime doesn't support it."

### `WaitForReady` Semantics

`WaitForReady` checks that the **runtime session exists** (e.g., `tmux has-session`), not that the agent is ready for input. "Session exists" and "agent ready" are distinct — the agent process may still be initializing. For post-launch plans that follow `WaitForReady` with `SendMessage`, agents must tolerate early input delivery. Claude Code handles this (input is buffered in the terminal). Agents that require explicit readiness confirmation should use output-pattern-based readiness checks (deferred to post-MVP).

### tmux Implementation

| RuntimeStep | tmux Command |
|-------------|-------------|
| `Create` | `tmux new-session -d -s {id} -c {dir} {command...}` |
| `WaitForReady` | Poll `tmux has-session -t {id}` until success or timeout |
| `SendMessage` | `tmux send-keys -t {id} {content} Enter` |
| `SendBuffer` | Write to temp file, `tmux load-buffer {file}`, `tmux paste-buffer -t {id}` |
| `SendProtocol` | `Err(UnsupportedStep)` |

**Session naming:** `{sessionPrefix}-{issueId}-{attempt}` (e.g., `myproject-42-1`). The lifecycle engine guarantees uniqueness by incrementing the attempt counter. Validated to be tmux-safe (`[a-zA-Z0-9._-]`).

**Output capture:** `tmux capture-pane -t {id} -p -S -{lines}` returns the last N lines of terminal output.

**CommandRunner:** All tmux commands go through the `CommandRunner` utility (ADR-0002 day-one task). This handles: spawning `tokio::process::Command`, capturing stdout/stderr, timeout enforcement, structured error wrapping.

### Deferred

| Feature | When | Why |
|---------|------|-----|
| `process` runtime | Fast-follow | tmux covers the primary use case; `process` is a lightweight fallback |
| `getMetrics()` | Post-MVP | Token tracking via agent JSONL, not runtime |
| Docker / K8s / E2B runtimes | Post-MVP | Listed as planned in PRD |

## 3. Agent Trait

The Agent defines what to run, how to interpret its output, and produces plans that drive the Runtime.

### Trait Signature

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    fn meta(&self) -> PluginMeta;

    /// Produce the launch sequence for this agent.
    fn launch_plan(&self, ctx: &LaunchContext) -> LaunchPlan;

    /// Parse gathered context into an activity state. Pure function — no I/O.
    fn detect_activity(&self, ctx: &GatherContext) -> ActivityState;

    /// Extract session metadata from gathered context. Pure function — no I/O.
    fn parse_session_info(&self, ctx: &GatherContext) -> AgentSessionInfo;

    /// Path to agent-specific auxiliary log (e.g., JSONL). Gatherer reads this.
    fn auxiliary_log_path(&self) -> Option<PathBuf> { None }

    // --- Post-MVP: optional with defaults ---

    /// Produce a continuation plan for multi-turn nudges.
    fn continue_plan(&self, ctx: &ContinueContext) -> ContinuePlan {
        ContinuePlan::default_send(ctx.message.clone())
    }

    /// Produce a restore plan for crashed sessions. None = not restorable.
    fn restore_plan(&self, ctx: &RestoreContext) -> Option<RestorePlan> {
        None
    }

    /// Produce a shutdown plan. Default: destroy runtime.
    fn shutdown_plan(&self, ctx: &ShutdownContext) -> ShutdownPlan {
        ShutdownPlan::default_destroy()
    }

    /// Agent-specific workspace hooks (distinct from config-driven FR2 hooks).
    fn workspace_hooks(&self, ctx: &WorkspaceHookContext) -> Vec<AgentWorkspaceHook> {
        vec![]
    }
}
```

Only `launch_plan`, `detect_activity`, and `parse_session_info` are required at MVP. Everything else has sensible defaults.

### GatherContext — Explicit I/O Boundary

The lifecycle engine's gatherer performs **all** I/O and populates a `GatherContext` that Agent methods consume as pure functions:

```rust
pub struct GatherContext {
    pub terminal_output: String,            // from runtime.get_output()
    pub auxiliary_log: Option<String>,       // agent-specific: JSONL content, protocol response
    pub auxiliary_log_path: Option<PathBuf>, // path the gatherer read from (diagnostics)
}
```

The gatherer calls `agent.auxiliary_log_path()` to discover what to read. Claude Code returns the JSONL log path; terminal-only agents return `None`. This keeps Agent detection methods pure and testable — construct a `GatherContext` literal, call `detect_activity()`, assert the result.

### Plan Types

Each lifecycle phase has its own plan type with phase-specific steps:

```rust
/// Launch sequence — produced by Agent, executed by lifecycle engine via Runtime.
pub struct LaunchPlan {
    pub steps: Vec<RuntimeStep>,
}

/// Continuation for multi-turn nudges (post-MVP).
pub struct ContinuePlan {
    pub steps: Vec<RuntimeStep>,
}

/// Restore a crashed session (post-MVP).
pub struct RestorePlan {
    pub steps: Vec<RuntimeStep>,
}

/// Graceful shutdown (post-MVP).
pub struct ShutdownPlan {
    pub steps: Vec<ShutdownStep>,
}

/// Shutdown has its own step type — it includes steps invalid for launch.
pub enum ShutdownStep {
    SendMessage { content: String },
    WaitForExit { timeout: Duration },
    ForceKill,
    DestroyRuntime,
}
```

`LaunchPlan`, `ContinuePlan`, and `RestorePlan` all use `Vec<RuntimeStep>` because the valid operations are the same. `ShutdownPlan` uses a dedicated `ShutdownStep` enum because it includes destructive operations (`ForceKill`, `DestroyRuntime`) that should be invalid in launch context.

**Why per-phase step enums matter:** Rust's exhaustive matching ensures `LaunchPlan` cannot contain `ForceKill` and `ShutdownPlan` cannot contain `Create`. This is exactly why ADR-0002 chose Rust — the compiler prevents invalid plan construction.

**`ShutdownStep` execution path.** `ShutdownStep` is **not** executed via `Runtime::execute_step()`. The lifecycle engine maps `ShutdownStep` variants to direct Runtime method calls: `SendMessage` → `runtime.execute_step(RuntimeStep::SendMessage { .. })`, `WaitForExit` → poll `runtime.is_alive()` with timeout, `ForceKill` → `runtime.destroy()`, `DestroyRuntime` → `runtime.destroy()`. This asymmetry is intentional — shutdown involves lifecycle-level escalation decisions that belong in the engine.

### LaunchContext

```rust
pub struct LaunchContext {
    pub prompt: String,
    pub workspace_path: PathBuf,
    pub session_id: String,
    pub agent_config: AgentConfig,
    pub env_extras: HashMap<String, String>, // AO_SESSION, AO_DATA_DIR, etc.
}
```

The Agent reads the context and decides the step sequence. The lifecycle engine does not interpret the plan — it hands steps to the Runtime.

### Activity State Detection

```rust
pub enum ActivityState {
    Active,       // Agent is processing
    Ready,        // Finished turn, waiting for input
    Idle,         // Inactive past readyThresholdMs
    WaitingInput, // Asking a question or requesting permission
    Blocked,      // Hit an error
    Exited,       // Process no longer running
}
```

The PRD specifies two strategies: JSONL log parsing (preferred) and terminal output parsing (fallback). Detection strategy is agent-specific.

**Important:** `Idle` is not detected by the Agent. The Agent reports `Ready`; the lifecycle engine's gatherer (ADR-0001) checks elapsed time and promotes `Ready` to `Idle` if past `readyThresholdMs`. This keeps timers out of the Agent trait, consistent with ADR-0001's decision that timer-based triggers are handled in the gather phase.

**Detection strategy per agent:**

| Agent | Primary Strategy | Mechanism |
|-------|-----------------|-----------|
| `claude-code` | JSONL | Parse `~/.claude/projects/.../logs/*.jsonl` for event types |
| `codex` | Terminal | Regex patterns on `runtime.get_output()` |
| `aider` | Terminal | Prompt patterns (`aider>` = ready, output streaming = active) |
| `opencode` | Terminal | TUI state patterns |
| `gemini` | Terminal | Prompt patterns |
| `openclaw` | Protocol | Query session state via ACP/JSON-RPC |

For JSONL-based agents (Claude Code), the implementation reads from `GatherContext.auxiliary_log`. No filesystem I/O in the Agent method:

```rust
fn detect_activity(&self, ctx: &GatherContext) -> ActivityState {
    // Primary: JSONL from auxiliary_log (read by gatherer)
    if let Some(ref log) = ctx.auxiliary_log {
        if let Some(state) = self.parse_jsonl(log) {
            return state;
        }
    }
    // Fallback: terminal output patterns
    self.parse_terminal_output(&ctx.terminal_output)
}

fn auxiliary_log_path(&self) -> Option<PathBuf> {
    // Claude Code's JSONL log location
    Some(self.jsonl_log_dir.clone())
}
```

### AgentSessionInfo

Agent-extracted metadata, written to session metadata files by the lifecycle engine. Renamed from `SessionInfo` to clarify this is agent-sourced, not the full session record (which also includes engine-sourced fields like `terminationReason`, action journal, etc.):

```rust
pub struct AgentSessionInfo {
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
}
```

`parse_session_info()` is called every poll tick (not just on output changes) so that token counts are fresh for budget evaluation (ADR-0001 `budgetExceeded` global edge).

### AgentWorkspaceHook

Agent-specific hooks are distinct from the four config-driven workspace hooks (`afterCreate`, `beforeRun`, `afterRun`, `beforeRemove`) in FR2. Config hooks are shell commands in YAML; agent hooks are programmatic callbacks registered by the Agent plugin:

```rust
pub enum AgentWorkspaceHook {
    /// Script injected as a Claude Code PostToolUse hook.
    PostToolUse { script: String },
    /// Script run after workspace creation (agent-specific setup).
    AfterCreate { script: String },
}
```

The lifecycle engine merges agent hooks with config hooks at workspace creation. Agent hooks run after config hooks.

### PromptDelivery Configuration

The PRD (FR1) specifies `promptDelivery` as a per-agent property. Added to `AgentConfig` as a typed field:

```rust
pub prompt_delivery: Option<PromptDelivery>,

pub enum PromptDelivery {
    Inline,     // prompt as CLI arg (default for most agents)
    PostLaunch, // prompt sent via runtime after agent starts
    Protocol,   // prompt sent via JSON-RPC / ACP
}
```

The Agent reads `ctx.agent_config.prompt_delivery` in `launch_plan()` to decide the step sequence.

### Claude Code Implementation

**Inline prompt delivery (default):**

```rust
fn launch_plan(&self, ctx: &LaunchContext) -> LaunchPlan {
    let mut command = vec!["claude".into()];
    let mut env = ctx.env_extras.clone();

    // Permissions
    if ctx.agent_config.permissions.as_deref() == Some("skip") {
        command.push("--dangerously-skip-permissions".into());
    }

    // Model override
    if let Some(ref model) = ctx.agent_config.model {
        command.extend(["--model".into(), model.clone()]);
    }

    // Max turns
    if let Some(turns) = ctx.agent_config.max_turns {
        command.extend(["--max-turns".into(), turns.to_string()]);
    }

    // Inline prompt
    command.extend(["-p".into(), ctx.prompt.clone()]);

    LaunchPlan {
        steps: vec![RuntimeStep::Create {
            command,
            env,
            working_dir: ctx.workspace_path.clone(),
        }],
    }
}
```

**Post-launch prompt delivery (for multi-turn interactive mode):**

```rust
// When agent_config indicates post-launch mode
fn launch_plan(&self, ctx: &LaunchContext) -> LaunchPlan {
    let command = vec!["claude".into()]; // No -p flag, interactive mode
    let env = ctx.env_extras.clone();

    LaunchPlan {
        steps: vec![
            RuntimeStep::Create {
                command,
                env,
                working_dir: ctx.workspace_path.clone(),
            },
            RuntimeStep::WaitForReady {
                timeout: Duration::from_secs(10),
            },
            RuntimeStep::SendMessage {
                content: ctx.prompt.clone(),
            },
        ],
    }
}
```

Same agent, different plan based on configuration. The lifecycle engine does not care — it executes the steps.

**Restore support:**

```rust
fn restore_plan(&self, ctx: &RestoreContext) -> Option<RestorePlan> {
    Some(RestorePlan {
        steps: vec![RuntimeStep::Create {
            command: vec!["claude".into(), "--continue".into()],
            env: ctx.env.clone(),
            working_dir: ctx.workspace_path.clone(),
        }],
    })
}
```

**Workspace hooks:**

```rust
fn workspace_hooks(&self, _ctx: &WorkspaceHookContext) -> Vec<AgentWorkspaceHook> {
    vec![AgentWorkspaceHook::PostToolUse {
        script: include_str!("hooks/claude_post_tool_use.sh").into(),
    }]
}
```

### MVP Scope for Agent Implementations

| Agent | MVP Status | Notes |
|-------|-----------|-------|
| `claude-code` | Full implementation | JSONL detection, inline + post-launch, restore, workspace hooks |
| `codex` | Stub | Factory returns `PluginError::NotImplemented` |
| `aider` | Stub | Same |
| `opencode` | Stub | Same |
| `gemini` | Stub | Same |
| `openclaw` | Stub | Same |

## 4. Lifecycle Engine Integration

The lifecycle engine (ADR-0001) uses these traits in three phases of its poll loop:

### Gather Phase

```
runtime.is_alive(session_id)                      → bool
runtime.get_output(session_id, lines)              → String (terminal_output)
read agent.auxiliary_log_path() from disk           → Option<String> (auxiliary_log)
build GatherContext { terminal_output, auxiliary_log, auxiliary_log_path }
agent.detect_activity(gather_ctx)                  → ActivityState
agent.parse_session_info(gather_ctx)               → AgentSessionInfo
```

Sequential per session (runtime → output → auxiliary log → activity → info), concurrent across sessions with bounded parallelism. The gatherer promotes `Ready` to `Idle` based on elapsed time (ADR-0001 timer handling). `parse_session_info()` is called every tick for fresh token counts (budget evaluation).

### Transition Phase

No direct plugin calls. Pure graph evaluation using gathered `PollContext`.

### Entry Action Phase

Side effects triggered by state transitions. Uses Runtime for cleanup:

```
killed  → runtime.destroy(session_id)
cleanup → runtime.destroy(session_id)  // workspace.destroy() handled by workspace plugin
```

### Session Spawn

When the lifecycle engine or CLI spawns a new session:

```
1. workspace = create_workspace(config)
2. agent = create_agent(config.agent, config.agent_config)
3. runtime = create_runtime(config.runtime, config)
4. plan = agent.launch_plan(launch_context)
5. validate_plan(plan, runtime)  // check supported_steps()
6. for step in plan.steps:
       result = runtime.execute_step(session_id, step)
       if result.is_err():
           log_plan_failure(plan, step_index, error)
           runtime.destroy(session_id)  // cleanup partial state
           set session status = "errored"
           return
7. Update session metadata: status = "spawning"
```

**Plan execution failure semantics:** On any step failure, the plan aborts immediately. If a `Create` step succeeded earlier, `runtime.destroy()` is called to reclaim the partially-created session. The session transitions to `errored` with the failing step index and error recorded in metadata. Each step is logged as a structured event (step index, type, duration, result) for debugging.

## 5. Module Structure

```
packages/core/src/
├── plugin/
│   ├── mod.rs          // PluginMeta, PluginError, validate_plugin_config()
│   └── registry.rs     // create_agent(), create_runtime(), etc.
├── runtime/
│   ├── mod.rs          // Runtime trait, RuntimeStep, RuntimeError
│   └── tmux.rs         // TmuxRuntime implementation
├── agent/
│   ├── mod.rs          // Agent trait, plans, ActivityState, SessionInfo
│   └── claude_code.rs  // ClaudeCodeAgent implementation
```

## 6. Plan Validation

Plans are validated before execution via `Runtime::supported_steps()`:

```rust
pub fn validate_plan(
    plan: &LaunchPlan,
    runtime: &dyn Runtime,
) -> Result<(), PlanValidationError> {
    let supported = runtime.supported_steps();
    for step in &plan.steps {
        if !supported.contains(&step.type_name()) {
            return Err(PlanValidationError::UnsupportedStep {
                step: step.type_name(),
                runtime: runtime.meta().name,
            });
        }
    }
    Ok(())
}
```

`RuntimeStep::type_name()` returns a static string (`"create"`, `"wait_for_ready"`, etc.) matching the values in `supported_steps()`. tmux returns all steps except `"send_protocol"`. This catches mismatches at session creation — before any steps execute — rather than mid-plan.

## 7. Testing Strategy

### Agent Plans (Unit Tests, No I/O)

```rust
#[test]
fn claude_code_inline_plan() {
    let agent = ClaudeCodeAgent::new(&default_config()).unwrap();
    let ctx = LaunchContext {
        prompt: "Fix the bug".into(),
        workspace_path: "/tmp/ws".into(),
        session_id: "test-1".into(),
        agent_config: AgentConfig { permissions: Some("skip".into()), ..default() },
        env_extras: HashMap::new(),
    };
    let plan = agent.launch_plan(&ctx);
    assert_eq!(plan.steps.len(), 1);
    match &plan.steps[0] {
        RuntimeStep::Create { command, .. } => {
            assert!(command.contains(&"-p".into()));
            assert!(command.contains(&"--dangerously-skip-permissions".into()));
        }
        _ => panic!("expected Create step"),
    }
}
```

### Activity Detection (Unit Tests, No I/O)

```rust
#[test]
fn claude_code_detects_active_from_jsonl() {
    let agent = ClaudeCodeAgent::new(&default_config()).unwrap();
    let ctx = GatherContext {
        terminal_output: String::new(),
        auxiliary_log: Some(r#"{"type":"tool_use","tool":"edit"}"#.into()),
        auxiliary_log_path: None,
    };
    assert_eq!(agent.detect_activity(&ctx), ActivityState::Active);
}

#[test]
fn claude_code_falls_back_to_terminal() {
    let agent = ClaudeCodeAgent::new(&default_config()).unwrap();
    let ctx = GatherContext {
        terminal_output: "╭─────────────────────────╮\n│ > waiting for input...  │".into(),
        auxiliary_log: None, // no JSONL available
        auxiliary_log_path: None,
    };
    assert_eq!(agent.detect_activity(&ctx), ActivityState::Ready);
}
```

### Runtime (Integration Tests, Requires tmux)

```rust
#[tokio::test]
async fn tmux_create_and_destroy() {
    let rt = TmuxRuntime::new(&default_config()).unwrap();
    rt.execute_step("test-session", &RuntimeStep::Create {
        command: vec!["echo".into(), "hello".into()],
        env: HashMap::new(),
        working_dir: PathBuf::from("/tmp"),
    }).await.unwrap();
    assert!(rt.is_alive("test-session").await.unwrap());
    rt.destroy("test-session").await.unwrap();
    assert!(!rt.is_alive("test-session").await.unwrap());
}
```

### Plan-Runtime Compatibility (Unit Tests)

Plans can be validated against runtime capabilities without executing them.

## 8. Summary of Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Plugin instantiation | Static factory functions with `match` dispatch | All plugins compiled in; exhaustive, zero-allocation |
| Agent-Runtime coupling | Zero — plans as intermediary | Decoupled via `RuntimeStep` enum; neither trait references the other |
| Launch flow | Declarative `LaunchPlan` produced by Agent | Lifecycle engine executes generically; new delivery modes don't change the engine |
| Runtime dispatch | Single `execute_step(RuntimeStep)` method | Uniform plan execution; new step types extend enum, not trait |
| Activity detection | Agent-specific via `GatherContext`, JSONL preferred, terminal fallback | Matches PRD strategy; gatherer reads all I/O, Agent methods are pure; `Idle` promoted by gatherer |
| Future plan types | `ContinuePlan`, `RestorePlan`, `ShutdownPlan` | Same pattern, phase-specific step enums; optional trait methods with defaults |
| MVP implementations | `claude-code` agent + `tmux` runtime | Default plugins per PRD |

## 9. Deferred

| Feature | When | Why |
|---------|------|-----|
| Non-claude-code agent implementations | Post-MVP | Stubs at MVP |
| `process` runtime | Fast-follow | tmux covers primary use case |
| Dynamic plugin loading | Post-MVP | No third-party plugins yet |
| `ContinuePlan` / `RestorePlan` / `ShutdownPlan` execution | Post-MVP | Trait methods defined with defaults; engine support lands with FR4 (reactions) |
| Output-pattern-based `WaitForReady` | Post-MVP | Current `WaitForReady` checks session existence only; pattern-based readiness deferred |
| Protocol-based agents (OpenClaw) | Post-MVP | `SendProtocol` step defined; no runtime support at MVP |
