# ADR-0001: Eight Slot Plugin Architecture

## Status

Draft

## Context

The Agent Orchestrator must support a rapidly evolving landscape of AI coding agents (Claude Code, Codex, Aider, OpenCode, Gemini) and varied infrastructure — different issue trackers (GitHub, Linear, Jira), runtimes (tmux, Docker, Kubernetes), and notification channels (Slack, desktop, webhooks). Adding support for a new agent or tracker should not require modifying core orchestration logic.

The system spans 8 distinct capability dimensions: Runtime (execution environment), Agent (AI logic), Workspace (filesystem isolation), Tracker (issue source), SCM (source control/PR/CI), Notifier (communication), Terminal (human-agent UI), and Lifecycle (orchestration coordination). Each dimension has multiple possible implementations with fundamentally different APIs and behaviors.

## Considered Options

1. **Monolithic core with built-in adapters** — All agent, runtime, and tracker logic lives in the core codebase behind conditional dispatch (if/switch). Simple to start, but every new integration touches core code. Testing requires mocking internals. The number of conditional branches grows combinatorially as integrations multiply.

2. **Strict slot-based plugin system** — Define 8 named slots, each with a well-defined interface. Plugins register via a manifest (name, slot, description, version) and a factory function. The core depends only on slot interfaces, never on concrete implementations. Slots are: Runtime, Agent, Workspace, Tracker, SCM, Notifier, Terminal, and Lifecycle (core, not pluggable).

3. **Generic middleware/hook pipeline** — A single event bus where plugins register handlers for lifecycle events. Maximum flexibility, but no structure — difficult to reason about which capabilities are present, no compile-time guarantees that required slots are filled, and handler ordering becomes implicit and fragile.

## Decision

Option 2 — Strict slot-based plugin system.

Each of the 8 slots defines a clear interface contract. Plugins are discovered by slot name and instantiated via their factory function. The core orchestrator only depends on these interfaces, making it agnostic to which specific agent, runtime, or tracker is in use.

The Lifecycle slot is an exception — it is the core orchestration logic and is not pluggable. The remaining 7 slots are fully swappable.

## Consequences

**Positive:**
- Clear extension points for community contributions — new agents, runtimes, or trackers can be added without touching core code.
- Each slot's interface serves as living documentation of the contract between core and plugins.
- Compile-time (or load-time) validation ensures required slots are filled before the orchestrator starts.
- Testing is straightforward — mock the slot interface, not internal implementation details.

**Negative:**
- More boilerplate per plugin (manifest + factory + full interface implementation), even for simple integrations.
- Adding a 9th slot (or removing one) requires changes to the core — the slot set is a structural decision.
- Slot interfaces must be designed carefully upfront. Changing an interface is a breaking change for every plugin in that slot.
