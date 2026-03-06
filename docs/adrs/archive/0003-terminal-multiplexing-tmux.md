# ADR-0003: Terminal Multiplexing with tmux

## Status

Draft

## Context

AI coding agents like Claude Code and Aider are CLI programs that read from stdin and write to stdout. The orchestrator needs to: (1) launch agents in persistent sessions that survive shell disconnects, (2) allow humans to "attach" to a running agent for debugging or manual intervention, (3) send messages — prompts, fix instructions, review feedback — to running agents programmatically, and (4) capture agent output for activity state detection.

These requirements rule out simple subprocess management. The runtime must provide session persistence, programmatic I/O, and an interactive attach/detach workflow.

## Considered Options

1. **tmux** — A mature terminal multiplexer with robust session management. Sessions survive shell disconnects and orchestrator restarts. Supports programmatic interaction via `tmux send-keys` (input) and `tmux capture-pane` (output). The native `tmux attach -t` workflow enables humans to inspect or take over a running agent. Widely available on Linux and macOS.

2. **GNU Screen** — Similar to tmux but older. Less scriptable — capturing pane output and sending keys programmatically is more cumbersome. Fewer features for session management and window layout. Community adoption is declining in favor of tmux.

3. **Direct process management (no multiplexer)** — Spawn agents as child processes with piped stdin/stdout. The simplest architecture with no external dependency. However, there is no human attach capability, sessions die when the orchestrator restarts, and there is no disconnect survival.

4. **Docker exec** — Run agents in containers and interact via `docker exec`. Provides strong OS-level isolation, but introduces heavy overhead, requires a running Docker daemon, and the attach user experience is poor compared to a native terminal multiplexer.

## Decision

Option 1 (tmux) as the primary local runtime, with Option 3 (direct process management) as a lightweight fallback for headless or CI environments.

tmux provides the best balance of programmability, session persistence, and human-in-the-loop debugging. The `process` runtime plugin exists for environments where tmux is unavailable or where the interactive attach workflow is unnecessary (e.g., automated pipelines).

## Consequences

**Positive:**
- Human attach workflow is native and well-understood (`tmux attach -t session-name`).
- Sessions survive orchestrator restarts — agents keep running even if the orchestrator crashes.
- Programmatic message delivery via `tmux send-keys` supports the `ao send` command, including buffer-based delivery for large messages.
- Output capture via `tmux capture-pane` enables JSONL-based and terminal-output-based activity state detection.

**Negative:**
- tmux is a hard dependency for the default runtime — it must be installed on the host machine.
- tmux scripting has quirks: send-keys timing issues, buffer size limits, and encoding edge cases require careful handling.
- The `process` runtime fallback loses the attach capability and disconnect survival, limiting its usefulness for interactive workflows.
