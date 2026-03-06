# PRD Review: Agent Orchestrator (Conductor) - Round 2
**Reviewer:** Codex
**Date:** March 6, 2026
**PRD Version:** 1.1

## Scope
Round-2 feature-soundness review focused on the AO + Symphony hybrid model and whether Round-1 risks were fully resolved.

## Strengths
- **Major improvement in control-plane clarity:** FR17 adds explicit ownership boundaries between worker agents and orchestrator lifecycle actions (`docs/prds/0001-agent-orchestrator.md:321`).
- **State determinism materially improved:** Section 5.3 introduces a precedence-based transition table as a single source of truth (`docs/prds/0001-agent-orchestrator.md:385`).
- **Idempotency now addressed at requirements level:** FR4/FR15 define an action journal and dedupe checks for destructive automation (`docs/prds/0001-agent-orchestrator.md:101`, `docs/prds/0001-agent-orchestrator.md:312`).
- **Budget controls added:** session token and wall-clock limits plus per-issue retry caps reduce runaway loop risk (`docs/prds/0001-agent-orchestrator.md:105`, `docs/prds/0001-agent-orchestrator.md:201`).

## Findings

### Critical
1. **Mechanical enforcement claim is currently bypassable through shell-level capabilities.**
- FR17 says lifecycle actions are mechanically prevented by withholding tools (`docs/prds/0001-agent-orchestrator.md:325-327`), but agent sessions still run in writable git workspaces with shell command execution implied throughout FR1/FR2/FR6 (e.g., `gh pr create`, `gh pr merge`, git operations in hooks and session workflows: `docs/prds/0001-agent-orchestrator.md:60`, `docs/prds/0001-agent-orchestrator.md:134`).
- Without command policy enforcement at runtime (allowlist/denylist, credential scoping, wrapper shell), workers can still perform lifecycle mutations directly, violating FR17 ownership guarantees.
- **Recommendation:** Add an explicit FR that enforces command- and credential-level policy (not just tool exposure), including blocked command families for worker sessions and scoped tokens for orchestrator-only mutations.

### High
2. **Budget semantics are internally inconsistent (`status` vs `reason`).**
- FR4 says sessions are auto-killed with a `budget_exceeded` status (`docs/prds/0001-agent-orchestrator.md:105`).
- Section 5.4 says transition is to `killed` with `budget_exceeded` reason (`docs/prds/0001-agent-orchestrator.md:429`).
- Session status list does not include `budget_exceeded` (`docs/prds/0001-agent-orchestrator.md:352-370`).
- **Recommendation:** Normalize to one model: keep `killed` as status and define `terminationReason=budget_exceeded` in the metadata schema.

3. **State-evaluation precedence is ambiguous between section logic and transition table.**
- Section 5.4 checks runtime liveness first (`docs/prds/0001-agent-orchestrator.md:428`), which can force `killed` before evaluating higher-priority `cleanup` transitions in some scenarios.
- Section 5.3 says transitions are evaluated by precedence numbers and are the single source of truth (`docs/prds/0001-agent-orchestrator.md:387`, `docs/prds/0001-agent-orchestrator.md:422`), where `any -> cleanup` is explicitly modeled (`docs/prds/0001-agent-orchestrator.md:419`).
- **Recommendation:** Define one authoritative evaluation algorithm and remove conflicting prose ordering; reference table precedence directly in 5.4.

4. **Action journal requirements are underspecified for retry correctness.**
- FR15 defines action type/target/timestamp/dedupe key (`docs/prds/0001-agent-orchestrator.md:312`) but omits outcome/result, error classification, and actor identity.
- Reaction retries/escalations depend on distinguishing transient failures from successful/no-op actions (`docs/prds/0001-agent-orchestrator.md:99-102`).
- **Recommendation:** Extend journal schema with `result` (`success|failed|skipped`), `errorCode`, `attempt`, and `actor` to support deterministic retries and auditability.

### Medium
5. **Security model is still deferred while enterprise-scale operation remains core narrative.**
- Security requirements for authn/authz, tool allowlisting, and audit logging are explicitly deferred (`docs/prds/0001-agent-orchestrator.md:453-459`), but target audience and autonomy claims assume broad team operation (`docs/prds/0001-agent-orchestrator.md:25-27`, `docs/prds/0001-agent-orchestrator.md:290`).
- **Recommendation:** Split MVP vs production-ready profiles in the PRD. Mark deferred security controls as required gates for multi-user/team rollout.

6. **Concurrency fairness still lacks starvation-prevention policy.**
- FR5 has global/per-state caps and priority sorting (`docs/prds/0001-agent-orchestrator.md:109-112`) but no fairness rule (aging, quota, max consecutive picks) for lower-priority issues.
- **Recommendation:** Add a dispatch fairness policy requirement (e.g., priority + aging score) and a measurable starvation bound.

## Final Assessment
The PRD is substantially stronger than Round 1 and addresses most foundational architecture concerns. Remaining issues are concentrated in **enforcement realism** (tool-level vs shell-level control) and **state/termination consistency**. Once those are tightened, the AO + Symphony hybrid model is sound enough to proceed into implementation planning with lower operational risk.
