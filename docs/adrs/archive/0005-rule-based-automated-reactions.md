# ADR-0005: Rule-Based Automated Reactions

## Status

Draft

## Context

When the lifecycle manager detects state changes — a CI failure, a reviewer requesting changes, an agent going idle — the system needs to respond. Some responses are mechanical: sending CI failure logs to the agent for a fix attempt, or forwarding review comments for the agent to address. Others require human judgment: deciding whether to merge an approved PR, or diagnosing why an agent is stuck.

Different teams have different tolerance levels. Some want aggressive auto-fix behavior where agents retry CI failures multiple times before escalating. Others prefer notification-only, keeping a human in the loop for every decision. The system handles 9 distinct reaction types: ci-failed, changes-requested, bugbot-comments, merge-conflicts, approved-and-green, agent-stuck, agent-needs-input, agent-exited, and all-complete. Each needs configurable retry limits and escalation thresholds.

## Considered Options

1. **Hardcoded event handlers** — Each event type has a fixed handler in the lifecycle manager. The simplest approach, but not configurable — every team gets the same behavior. Adding a new reaction type or changing escalation logic requires code changes. Difficult to support per-project customization.

2. **Declarative reaction rules** — A `reactions` configuration maps event types to actions. Three action types are supported: `send-to-agent` (forward information for the agent to act on), `notify` (alert a human via configured notifier channels), and `auto-merge` (merge an approved, CI-green PR). Each rule specifies retry limits and escalation thresholds. Rules are configurable globally and overridable per-project. A reaction tracker maintains per-session attempt counts to prevent infinite retry loops.

3. **User-defined scripts per event** — Each event triggers a user-provided shell script or webhook URL. Offers maximum flexibility, but requires users to write and maintain their own automation scripts. Error handling, retry logic, and escalation fall entirely on the user. Difficult to provide sensible defaults that "just work."

## Decision

Option 2 — Declarative reaction rules.

Sensible defaults cover all 9 reaction types out of the box. Per-project overrides allow teams to customize escalation thresholds, disable specific reactions, or switch from auto-fix to notification-only without forking. The three action types (`send-to-agent`, `notify`, `auto-merge`) cover the vast majority of use cases.

Option 3 (user-defined scripts) could be added later as a fourth action type (`run-script`), extending the declarative system rather than replacing it.

## Consequences

**Positive:**
- Decouples event detection from response logic — the lifecycle manager detects state changes, the reaction engine decides what to do. Each concern is independently testable.
- Retry counting and escalation thresholds prevent infinite loops (e.g., an agent repeatedly failing to fix a CI issue).
- Per-project overrides support diverse team workflows within the same orchestrator instance.
- Defaults work out of the box with no configuration required.

**Negative:**
- The reaction configuration can grow complex for advanced use cases with many per-project overrides.
- The action vocabulary is limited to 3 types. Teams needing custom automation must wait for a `run-script` action type or work around it.
- Escalation thresholds are time-based, which may not suit scenarios where escalation should be triggered by other signals (e.g., number of file changes or test count).
