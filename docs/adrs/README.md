# Architecture Decision Records

This directory contains the Architecture Decision Records (ADRs) for the Agent Orchestrator project.

## Format

Each ADR follows a hybrid format combining [Michael Nygard's original](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions) with [MADR's](https://adr.github.io/madr/) "Considered Options" section:

- **Status** -- Draft, Proposed, Accepted, Deprecated, or Superseded
- **Context** -- Problem and forces at play
- **Considered Options** -- Alternatives evaluated with trade-offs
- **Decision** -- What was chosen and why
- **Consequences** -- What becomes easier or harder

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [ADR-0001](0001-session-lifecycle-engine.md) | Session Lifecycle Engine | Accepted |

## Layered Approach

ADRs are organized in layers. This first layer contains **foundational** decisions that gate downstream choices:

- **Layer 1 (this set):** Core architecture, isolation strategy, runtime, lifecycle, reactions, persistence, and implementation language.
- **Layer 2 (future):** CLI framework, config validation library, dashboard framework, mobile framework, test framework, real-time transport, monorepo structure. These depend on the implementation language decision (ADR-0007).
