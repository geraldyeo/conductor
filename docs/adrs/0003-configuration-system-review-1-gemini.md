# ADR-0003 Review — Round 1 (Gemini)

**ADR version reviewed:** 0003-configuration-system.md as of 2026-03-06 (status: Proposed)
**Design doc reviewed:** docs/plans/2026-03-06-configuration-system-design.md

## Strengths

1. **Sound option analysis.** The three options (monolithic, layered merge, schema-driven) are well-characterized, and the rationale for choosing Option 1 is compelling. The argument that dynamic schema validation works against ADR-0002's explicit-traits philosophy is particularly well-reasoned.

2. **Forward-compatibility seam.** The `#[serde(flatten)]` on `AgentConfig.extra` and `Option<Value>` for post-MVP fields is a pragmatic approach. Config files written today will not break as features land. This is a real usability win for early adopters.

3. **Two-pass validation is well-structured.** Separating structural (serde) from semantic (garde) validation is clean. The error types in the design doc (with file path, field path, and human-readable message) will produce excellent terminal output.

4. **Small, focused public API.** Four functions is appropriate for MVP. The `load()` / `load_from_path()` split cleanly separates normal operation from testing and `ao init --check`.

5. **Secrets isolation.** Putting secrets in a separate `ResolvedSecrets` struct with no `Serialize` derive is a good defensive measure against accidental exposure.

6. **Explicit deferred-feature table.** The ADR and design doc both clearly enumerate what is deferred and why, making scope boundaries unambiguous.

7. **Ownership model with upgrade path.** The `Arc<Config>` now, `ArcSwap<Config>` later strategy is a clean progression that avoids premature complexity.

## Findings

### Critical

None.

### High

**H1. Config discovery order diverges from PRD — `startDir` is missing.**

The PRD (FR10, lines 191-195) specifies a 4-step search order:
1. `AO_CONFIG_PATH` environment variable
2. Walk up directory tree from CWD
3. **Explicit `startDir` parameter**
4. Home directory fallback

The ADR and design doc both specify a 3-step order that omits step 3 (`startDir`). The design doc's deferred table mentions `startDir` as deferred to "FR6 (CLI ADR)" with the note "CLI flag, not config system concern."

However, the PRD places `startDir` *between* CWD walk-up and home-dir fallback in the discovery chain itself — it is not merely a CLI flag but an alternative root for directory traversal. If a user sets `startDir`, discovery should walk up from that directory instead of (or in addition to) CWD. This is a semantic difference: deferring the CLI flag is fine, but the discovery function's signature should accept an optional `start_dir: Option<&Path>` parameter so the CLI can pass it through when FR6 lands. Without this, the `discover_config_path()` API will need a breaking signature change later.

**Recommendation:** Add an optional `start_dir` parameter to `discover_config_path()` now (defaulting to `std::env::current_dir()` when `None`). This is a one-line change that preserves forward-compatibility. Document the PRD's 4-step order and note that the CLI does not expose the parameter yet.

**H2. `maxConcurrentAgentsByState` is missing from the schema.**

The PRD top-level options table includes `maxConcurrentAgentsByState` (type: `Record`, default: `{}`). The ADR's deferred table mentions it, and the design doc includes it in the deferred table as well. However, it is not present as a field in the `Config` struct in the design doc (Section 1, lines 27-65). Every other post-MVP field appears as `Option<Value>` or `Option<T>` in the struct — this one is simply absent.

If a user adds `maxConcurrentAgentsByState: { "In Progress": 3 }` to their config file today, serde will reject it as an unknown field (assuming `deny_unknown_fields` is enabled) or silently drop it. The ADR's own principle is that post-MVP fields should "deserialize without error" via `Option<Value>`. This field should follow the same pattern.

**Recommendation:** Add `pub max_concurrent_agents_by_state: Option<Value>` to the `Config` struct, consistent with how other post-MVP fields are handled.

### Medium

**M1. Path validation with tilde: failure mode unspecified.**

The ADR states `path` has a custom validator with tilde expansion (line 71). The design doc lists `shellexpand` as a dependency (line 466). However, neither document specifies what happens when:
- Tilde expansion fails (e.g., `HOME` is unset, or the path uses `~otheruser/` on a system without that user)
- The expanded path contains symlinks — should the validator resolve them, or check the literal path?
- The path is a relative path without tilde (e.g., `./projects/myapp`) — is this accepted or rejected?

These are edge cases that will surface in real usage, especially in CI environments where `HOME` may be nonstandard.

**Recommendation:** Specify in the validation section: (a) tilde expansion failure produces a `ConfigError::Validation` with a clear message, (b) symlinks are not resolved (check literal expanded path), (c) relative paths are resolved against the config file's parent directory (a common convention in tools like `tsconfig.json`). If relative paths are intentionally rejected, document that explicitly.

**M2. `serde(deny_unknown_fields)` not specified.**

The ADR and design doc do not state whether the `Config` struct uses `#[serde(deny_unknown_fields)]`. This matters:
- If enabled: typos in config field names produce clear errors (good), but post-MVP fields not captured by `Option<Value>` will be rejected (bad — contradicts the forward-compatibility goal).
- If disabled: typos are silently ignored (bad UX).

The presence of explicit `Option<Value>` fields for known post-MVP options suggests `deny_unknown_fields` is *not* intended. But truly unknown fields (typos, or fields from a newer version of the orchestrator) will then be silently dropped.

**Recommendation:** Explicitly state the policy. Consider enabling `deny_unknown_fields` on nested structs (where the field set is stable) but not on `Config` or `ProjectConfig` (where new fields will be added). Alternatively, capture remaining fields via `#[serde(flatten)] pub extra: HashMap<String, Value>` at the top level and log a warning for any keys found there.

**M3. `AO_SESSION` and `AO_DATA_DIR` environment variables not addressed.**

The PRD's environment variable table (line 256-257) lists `AO_SESSION` and `AO_DATA_DIR` as automatically-set per-session env vars. Neither the ADR nor the design doc mentions them. While these are "set" rather than "read" by the config system, they are part of FR10's specification. The design should at least acknowledge them and clarify that they are the lifecycle engine's responsibility (not config loading).

**Recommendation:** Add a brief note in the ADR or design doc stating that `AO_SESSION` and `AO_DATA_DIR` are set by the runtime/lifecycle engine per session and are not part of the config loading pipeline.

**M4. No `#[serde(default)]` on `AgentConfig` struct itself.**

In the design doc (line 164), `AgentConfig` derives `Deserialize` but not `Default`. The `ProjectConfig` field is `pub agent_config: Option<AgentConfig>` (line 122), which handles absence at the project level. However, if a user writes:

```yaml
agentConfig: {}
```

Then serde will attempt to deserialize an `AgentConfig` from an empty mapping. Fields like `max_turns` and `sandbox` have `#[serde(default = "...")]` so they will get defaults. But `permissions`, `model`, and `extra` are `Option<T>` / `HashMap` with no `#[serde(default)]`, so `extra` may fail on an empty map depending on serde_yml's behavior with `#[serde(flatten)]`. This should be tested or documented.

**Recommendation:** Either derive `Default` for `AgentConfig` or verify that all fields handle the empty-mapping case correctly. Add a test case for `agentConfig: {}` in the design doc's test plan.

**M5. Plugin name validation is hard-coded — tension with plugin extensibility.**

The ADR (line 73) and design doc (line 298) specify that plugin names are validated against known values (e.g., runtime in `{"tmux", "process"}`). This is sound for MVP but creates a maintenance burden: every new plugin requires a core recompile and release. It also means third-party or experimental plugins cannot be used without modifying core.

**Recommendation:** Acknowledge this as a known limitation in the Consequences section. Consider adding a `--skip-plugin-validation` flag or an `allowUnknownPlugins: true` config option for future extensibility.

### Low

**L1. `generate_default()` signature could accept more parameters.**

The current signature is `generate_default(project_id, repo, path) -> String`. The generated YAML (design doc line 416-432) also includes `port`, `maxConcurrentAgents`, and `defaults`. These are hard-coded in the function. If `ao init` later wants to accept `--agent codex` or `--runtime process`, the signature will need to change.

**Recommendation:** Consider accepting a builder struct or partial config rather than individual parameters. Low priority since `ao init` is deferred to the CLI ADR.

**L2. The ADR claims "42 config options" (line 13) but the actual count is approximate.**

Counting the PRD tables: 18 top-level + 23 per-project = 41 YAML options, or 45 if agentConfig sub-fields (maxTurns, permissions, model, sandbox) are counted individually. The "42" figure is close but not exact. This is cosmetic but could cause confusion during audits.

**Recommendation:** Either verify the exact count or soften the language to "roughly 40 config options."

**L3. No mention of YAML anchors or aliases.**

YAML supports anchors (`&`) and aliases (`*`) for config reuse across projects. Users may attempt to use them. `serde_yml` supports this, but the behavior with `garde` validation (which runs post-deserialization) should be fine. Still, worth a brief note that YAML anchors are supported by virtue of `serde_yml`.

**L4. Home directory fallback includes only `.yaml` for the XDG path.**

The home directory fallback checks `~/.agent-orchestrator.yaml`, `~/.agent-orchestrator.yml`, and `~/.config/agent-orchestrator/config.yaml` — but not `~/.config/agent-orchestrator/config.yml`. The CWD walk-up checks both `.yaml` and `.yml` extensions. This inconsistency could surprise users who prefer `.yml`.

**Recommendation:** Add `~/.config/agent-orchestrator/config.yml` to the fallback list for consistency.

## Summary

ADR-0003 is a well-structured, pragmatic design that plays to Rust's strengths and aligns closely with the PRD. The monolithic typed struct is the right call for MVP, and the forward-compatibility seams (`Option<Value>`, `serde(flatten)`) are thoughtfully placed.

The two High findings should be addressed before acceptance:
- **H1** (missing `start_dir` parameter) creates a future API break that is trivially avoidable now.
- **H2** (missing `maxConcurrentAgentsByState` field) is an oversight that contradicts the ADR's own forward-compatibility principle.

The Medium findings are design clarifications that would strengthen the ADR but are not blocking. The Low findings are minor polish items.

**Recommendation:** Address H1 and H2, then accept.
