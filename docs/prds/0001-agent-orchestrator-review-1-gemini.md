# PRD Review: Agent Orchestrator (Conductor)
**Reviewer:** Gemini CLI
**Date:** Friday, 6 March 2026

## 1. Executive Summary of Analysis
The `docs/PRD.md` represents a sophisticated synthesis of two primary AI orchestration patterns: the **Orchestrator-led** model (e.g., ComposioHQ's `ao`) and the **Agent-led** model (e.g., OpenAI's `Symphony`). The architecture is structurally sound, focusing on isolation, parallel execution, and a multi-layered lifecycle.

The core innovation is the **Hybrid Control Plane**:
- **Orchestrator-led:** Manages the external PR/CI/Tracker lifecycle through a state machine and a reaction engine.
- **Agent-led:** Empowers individual agents via multi-turn sessions (up to 20 turns) and direct tool access (`tracker_graphql`) to self-manage their progress.

---

## 2. Deep Dive: Key Architectural Strengths

### A. Multi-Turn Sessions vs. Single-Shot Reactions (FR1)
Moving from single-shot prompts to **multi-turn sessions** is a critical efficiency play. 
- **Soundness:** Keeping the agent "alive" in a tmux session across 20 turns allows it to maintain local shell state, git history, and uncommitted fixes.
- **Integration:** The PRD correctly identifies the need to "re-check issue state between turns," effectively turning the agent into its own micro-orchestrator for the duration of its task.

### B. The "Orchestrator-as-Session" (FR13)
This is the most advanced "Symphony-style" feature. By making the orchestrator itself an AI agent, the system gains a flexible "Manager" layer.
- **Soundness:** It moves complex scheduling (priority-based dispatch, dependency awareness) from rigid code into a reasoning-capable agent.
- **Potential:** This agent can handle "vague" instructions like "Stabilize the CI for the whole project" by spawning multiple worker agents and coordinating their outputs.

### C. Workspace Isolation (FR2)
The decision to use `git worktree` is the "gold standard" for this use case.
- **Soundness:** It provides the filesystem isolation of a full clone with the speed and disk efficiency of a single repository.
- **Practicality:** The addition of a symlink strategy for shared config (`.env`, `.claude`) is a necessary "quality of life" feature for AI agents that require credentials or local context.

### D. Status vs. Activity State Separation (Section 5)
The PRD's distinction between **Session Status** (external state like `review_pending`) and **Activity State** (internal state like `active` or `waiting_input`) is a major architectural strength. It allows the system to remain responsive even when the agent is "thinking" or the CI is "running."

---

## 3. Identified Gaps & Refinement Opportunities

### 1. The "Cooperation Protocol" (Conflict Resolution)
There is a potential race condition between the **Reaction Engine (FR4)** and the **Multi-Turn Session (FR1)**.
- **Problem:** If an agent is in turn 5 of 20 and a CI failure occurs, should the orchestrator interrupt immediately (nudge) or wait for the turn to end?
- **Recommendation:** Implement a protocol where the orchestrator only triggers a Reaction if the agent reaches a `ready` or `idle` state without addressing the new event, or if the event is "terminal" (e.g., issue closed in tracker).

### 2. Cost Management & Budget Hard-Stops
While FR7 tracks tokens, it lacks an automated "kill switch."
- **Recommendation:** Add a `maxSessionCost` or `maxSessionTokens` configuration. If an agent enters a "loop" and consumes $50 on a single bug, the orchestrator must terminate it automatically to prevent "cost explosions."

### 3. Inter-Agent Conflict Detection
While worktrees prevent *filesystem* conflicts, they don't prevent *logical* or *merge* conflicts.
- **Recommendation:** The **Orchestrator-Agent (FR13)** should have a tool to "Scan Active Session Files." If two agents are touching the same file, the orchestrator-agent should either sequence them or warn the human.

### 4. Human-in-the-Loop UX (FR4/FR6)
The `needs_input` state is vital, but the UX for providing that input via `ao send` needs to be "bulletproof."
- **Recommendation:** Ensure the `ao send` command handles "interactive prompt" detection (e.g., if the agent is at a `(y/n)` prompt, `ao send` must be able to deliver the key without the agent's internal parser rejecting it as a new "prompt").

---

## 4. Final Verdict
The PRD is **highly sound and ready for implementation**. It successfully avoids the "dumb task-runner" trap by incorporating Symphony's autonomous agent principles, while maintaining the "enterprise safety" of the orchestrator-led `ao` model.

**Next Immediate Step:** Finalize **ADR-0007 (Implementation Language)**. The complexity of managing parallel tmux sessions, GitHub GraphQL, and a web dashboard with real-time SSE updates makes **Rust** (with the `tokio` runtime) the most robust choice for this architecture.
