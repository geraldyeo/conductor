# PRD Review (By Codex)

## Scope
Feature-soundness review of the PRD, with emphasis on the combined AO + Symphony model.

## Findings

1. **Critical: Dual-control-plane conflict is not resolved.**  
The PRD says the orchestrator actively manages PR/CI/review lifecycle, while agents can also self-manage tracker state via dynamic tools. There is no explicit authority model for conflicting writes (agent vs orchestrator vs human), so race conditions are guaranteed in real use.  
Evidence: `docs/PRD.md:7`, `docs/PRD.md:83`, `docs/PRD.md:256`, `docs/PRD.md:277`

2. **Critical: No idempotency spec for destructive automations.**  
Actions like close PR, auto-merge, restart from fresh branch, and cleanup can be retriggered by polling/retries without dedupe keys or action journals. That can cause repeated PR churn and branch damage.  
Evidence: `docs/PRD.md:96`, `docs/PRD.md:103`, `docs/PRD.md:359`, `docs/PRD.md:366`

3. **High: Cost/safety guardrails are incomplete for autonomous loops.**  
Multi-turn + continuation retries + reaction retries are defined, but there is no global per-issue budget (tokens, wall-clock, max restarts). This can silently run expensive loops.  
Evidence: `docs/PRD.md:40`, `docs/PRD.md:41`, `docs/PRD.md:87`, `docs/PRD.md:107`

4. **High: State model is not fully deterministic.**  
16 session statuses + 6 activity states are listed, but transition precedence is underspecified (for example `waiting_input` vs `ci_failed`, or `idle` vs `stuck`). This will create inconsistent reaction behavior.  
Evidence: `docs/PRD.md:316`, `docs/PRD.md:336`, `docs/PRD.md:349`

5. **High: Scheduler fairness/duplication rules are missing.**  
Priority + per-state limits are good, but no lease/claim protocol is defined for issue dispatch, especially across restart recovery. Duplicate session creation for one issue is likely under race.  
Evidence: `docs/PRD.md:108`, `docs/PRD.md:109`, `docs/PRD.md:310`, `docs/PRD.md:367`

6. **High: Security boundaries are too open for enterprise use.**  
Shell hooks, protocol-based agent channels, dynamic tools, and optional `sandbox: "full"` are powerful but there is no permission/allowlist/audit policy for risky actions (merge, close, tracker mutation, shell execution).  
Evidence: `docs/PRD.md:46`, `docs/PRD.md:55`, `docs/PRD.md:39`, `docs/PRD.md:256`

7. **Medium: Dashboard/terminal/mobile control surfaces lack authn/authz requirements.**  
WebSocket terminal + session mutation APIs + mobile app are defined, but there are no explicit authentication, RBAC, or secret-handling requirements.  
Evidence: `docs/PRD.md:140`, `docs/PRD.md:146`, `docs/PRD.md:169`

8. **Medium: Scope is likely too broad for a first integrated AO+Symphony release.**  
Core orchestrator, dashboard, terminal UI, mobile app, onboarding, multi-agent protocol, and orchestrator-as-agent all in one PRD increases delivery risk and architecture churn before stabilization.  
Evidence: `docs/PRD.md:137`, `docs/PRD.md:158`, `docs/PRD.md:169`, `docs/PRD.md:275`

## What Is Sound
- Plugin slot decomposition is strong and extensible.
- Workspace isolation + lifecycle hooks are practical.
- Polling-based lifecycle is coherent with local-first operation.
- Token observability and reaction framework are good foundations.

## Most Important PRD Fixes Before Implementation
1. Define a strict **authority model**: who can mutate tracker/PR/session state, and conflict resolution order.
2. Add **idempotency + action journal** requirements for every automated mutation.
3. Add **budget caps**: max tokens/session, max retries/issue/day, max wall-clock/issue.
4. Specify a **deterministic state-transition table** (single source of truth).
5. Add **security policy** for tools/hooks/APIs: authn, RBAC, allowlists, audit logs.
