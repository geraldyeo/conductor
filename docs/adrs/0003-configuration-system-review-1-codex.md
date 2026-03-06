# ADR-0003 Review — Round 1 (Codex)

**ADR version reviewed:** 0003-configuration-system.md (2026-03-06, Status: Proposed)
**Design doc reviewed:** docs/plans/2026-03-06-configuration-system-design.md

## Strengths

1. **Sound option analysis.** The rejection of Option 2 (Layered Merge) and Option 3 (Schema-Driven) is well-reasoned. The monolithic typed struct plays to Rust's strengths and avoids the partial-struct boilerplate explosion that layered merging would introduce. The argument against JSON Schema validation aligns with ADR-0002's rationale for choosing Rust (compile-time safety over runtime validation).

2. **Clean two-pass validation pipeline.** Separating structural validation (serde) from semantic validation (garde) gives clear, actionable error messages. The terminal output example (design doc Section 3) demonstrates good UX with field paths and human-readable messages.

3. **Forward-compatibility strategy is pragmatic.** Using `Option<Value>` for post-MVP fields and `#[serde(flatten)]` for agent extras is a reasonable approach that prevents config file breakage as features land incrementally.

4. **Secrets isolation.** The `ResolvedSecrets` struct with no `Serialize` derive is a simple, effective guard against accidental secret leakage. This is a good pattern.

5. **Small public API surface.** Four functions is appropriately minimal. The split between `load()` and `load_from_path()` supports both production and test use cases cleanly.

6. **Config discovery follows established conventions.** Walk-up-tree discovery mirrors git/npm/cargo behavior, reducing cognitive load for users.

7. **ArcSwap upgrade path.** Calling out `ArcSwap<Config>` for hot-reload post-MVP shows the design has been future-proofed without over-engineering the MVP.

## Findings

### Critical

None.

### High

**H1. `serde_yml` stability risk.**
ADR-0002 describes `serde_yml` as the "maintained successor to archived `serde_yaml`." While accurate, `serde_yml` (crate by `sebastienrousseau`) is a relatively young fork. As of early 2026, it has significantly fewer downloads and fewer maintainers than the original `serde_yaml` had at its peak. The `unsafe-libyaml` backend it uses is maintained, but the top-level crate has had periods of slow response to issues.

**Risk:** If `serde_yml` falls behind on `serde` compatibility or introduces regressions, the config system is blocked. There is no drop-in alternative — `serde_yaml` is archived.

**Recommendation:** (a) Add `serde_yml` version pinning to the design doc's crate table with a minimum tested version. (b) Note the fallback plan: if `serde_yml` stalls, the project can vendor the crate or switch to a two-pass approach (parse YAML to `serde_json::Value` via a different YAML parser like `yaml-rust2`, then deserialize from Value via `serde_json::from_value`). This should be documented as a known risk, not necessarily acted on now.

**H2. `#[serde(flatten)]` has known edge cases with non-self-describing formats.**
`serde(flatten)` works by collecting unknown fields into a map. With `serde_yml`, there are known interactions where `flatten` combined with `rename_all` can produce surprising behavior — particularly around camelCase keys that partially match known fields. Additionally, `flatten` disables certain serde optimizations and can produce confusing error messages when deserialization of the flattened map fails (the error points to the container struct, not the specific key).

**Risk:** Users adding agent-specific extra fields may get confusing error messages if a value type is wrong. A field like `maxTokens: "not-a-number"` in `AgentConfig.extra` would deserialize as `Value::String` without error (since it goes into `HashMap<String, Value>`), but a plugin expecting a number would fail later with a less actionable error.

**Recommendation:** (a) Document this behavior explicitly — `extra` fields are unvalidated until FR1 lands. (b) Consider adding a `validate_extras()` hook point on `AgentConfig` that plugins can implement post-FR1, and note this in the ADR's deferred section. (c) Add an integration test specifically for `flatten` + `rename_all` + `serde_yml` to catch regressions early.

**H3. Missing `maxConcurrentAgentsByState` from the schema.**
The PRD (FR10, line 206) specifies `maxConcurrentAgentsByState` as a top-level config option of type `Record` (per-tracker-state concurrency limits). The ADR's deferred table mentions it, but the design doc's `Config` struct does not include it — not even as an `Option<Value>`. This means a user who includes `maxConcurrentAgentsByState` in their YAML will get a serde deserialization error (unknown field), contradicting the stated goal that "users can add fields before their FRs land without breaking validation."

**Recommendation:** Add `pub max_concurrent_agents_by_state: Option<Value>` to the `Config` struct in the design doc, consistent with how `notifiers`, `notification_routing`, and `reactions` are handled.

### Medium

**M1. No handling of YAML duplicate keys.**
The YAML 1.2 spec allows duplicate keys but leaves behavior implementation-defined. `serde_yml` (via `unsafe-libyaml`) silently takes the last value for duplicate keys. This can lead to confusing behavior: a user who accidentally duplicates a `projects` key will silently lose the first project definition.

**Recommendation:** Note this as a known limitation. Post-MVP, the config linter (`ao config check`) should parse the raw YAML AST and warn on duplicate keys before handing off to serde.

**M2. Permission-denied and I/O error paths are underspecified.**
The `ConfigError::Io` variant exists but the design doc's `discover_config_path()` pseudocode does not handle permission-denied errors during file reads. If the config file exists but is not readable (e.g., `chmod 000`), the current code would return `Ok(path)` from discovery but fail during `std::fs::read_to_string()`. The resulting error would be a raw `std::io::Error` that needs to be wrapped with the file path for a good user experience.

**Recommendation:** Ensure the loading pipeline (between discovery and serde parsing) wraps `std::fs::read_to_string` errors into `ConfigError::Io { path, message }` with the resolved path. The pseudocode in the design doc should show this explicitly.

**M3. `validate_path_exists` runs at load time — fragile for portable configs.**
The custom validator checks that `path` exists on disk during config validation. This means a config file that references a project path on a different machine (e.g., shared team config) will fail validation even if the user only intends to work with a subset of projects. It also means validation is side-effecting (touches the filesystem), which complicates testing.

**Recommendation:** (a) Consider making path validation a warning rather than an error, or (b) only validate paths for projects the user actually operates on (deferred to CLI layer), or (c) at minimum, document this as a known constraint and note the testing implication (tests need real or mocked paths). Option (b) is cleanest but requires the CLI to perform per-project validation, which shifts responsibility away from the config module.

**M4. Config module placement — `packages/core/src/config/` vs. standalone crate.**
The design places config as a module within `core`. As the project grows, `core` will contain the lifecycle engine, config, plugin traits, and shared types. Config is a leaf dependency (everything depends on it, it depends on nothing internal). Making it a standalone crate (`packages/config/`) would enable faster incremental compilation (changes to lifecycle engine don't recompile config's dependents), clearer dependency direction, and the ability for the CLI crate to depend on config without pulling in the full core.

**Recommendation:** Consider extracting config into its own workspace crate (`ao-config`). This is not blocking — it can be done during implementation if the `core` crate grows unwieldy — but it is worth noting as a likely refactor.

**M5. `generate_default()` returns `String` — no structured generation.**
The `generate_default()` function returns a raw YAML string. This means the generated output is a hand-crafted template, not a serialization of a `Config` struct. If the schema evolves, the template and the struct can drift apart silently.

**Recommendation:** Consider generating the default config by constructing a `Config` struct with default values and serializing it via `serde_yml::to_string()`, then optionally post-processing to add comments. This keeps the generated output in sync with the schema. If comments in the generated YAML are important (they usually are for `init` output), note that `serde_yml` does not support comment preservation, so the hand-crafted template approach may be necessary — but add a test that deserializes the generated output to ensure it round-trips.

**M6. `Defaults.notifiers` default value mismatch with PRD.**
The PRD (FR10, line 214) specifies `defaults.notifiers` defaults to `["composio", "desktop"]`. The design doc's `Defaults` struct uses `#[serde(default)]` on `notifiers: Vec<String>`, which defaults to an empty vector. This is a minor schema/PRD mismatch.

**Recommendation:** Either add a `default_notifiers()` function returning `vec!["composio".into(), "desktop".into()]` or document that notifiers are post-MVP and the empty default is intentional.

### Low

**L1. `garde` crate maturity.**
`garde` is a solid choice for declarative validation, but it is less established than `validator` (the older alternative). As of early 2026, `garde` has good momentum and a cleaner API, but it is worth pinning the version and noting the alternative in case `garde` development stalls.

**L2. Tilde expansion library choice.**
The design doc lists `shellexpand` for tilde expansion. This is a small, focused crate — confirm it handles edge cases like `~otheruser/path` (which may not be relevant for this project but is standard shell behavior). `dirs` is already a dependency for home directory resolution; ensure `shellexpand` and `dirs` agree on what `~` expands to.

**L3. `ProjectConfig.path` is `String`, not `PathBuf`.**
Using `String` for file paths works but loses the type-level distinction between paths and arbitrary strings. After tilde expansion, converting to `PathBuf` early (perhaps in `resolve_defaults()`) would provide better ergonomics for downstream consumers and catch path issues sooner.

**L4. No `Display` or `Debug` sanitization on `Config`.**
`Config` derives `Debug`, which means `println!("{:?}", config)` will dump the entire config including potentially sensitive fields (e.g., `AgentConfig.extra` might contain tokens if a user puts them there by mistake). Consider either implementing a custom `Debug` that redacts `extra` fields, or relying on the fact that secrets are in `ResolvedSecrets` and documenting that `Config` should not contain secrets.

**L5. PRD search order includes `startDir` parameter (item 3) that the ADR omits.**
The PRD config search order (line 194) lists `startDir` as step 3 between CWD walk-up and home directory fallback. The ADR correctly notes this is deferred to FR6 (CLI), but it would be clearer to mention the omission explicitly in the deferred table rather than silently skipping it.

The design doc does include `startDir` in the deferred table (row: "`startDir` CLI parameter | FR6 (CLI ADR)"), but the ADR itself does not. Adding a brief note to the ADR's deferred table would improve traceability.

## Summary

ADR-0003 is well-structured, makes defensible choices, and integrates cleanly with ADR-0001 and ADR-0002. The monolithic typed struct is the right call for MVP, and the forward-compatibility strategy is sound.

The three High findings should be addressed before accepting:
- **H1** (serde_yml stability): Document the risk and fallback plan.
- **H2** (serde flatten edge cases): Add test coverage and document the unvalidated-extras behavior.
- **H3** (missing maxConcurrentAgentsByState): Add the `Option<Value>` field to maintain the forward-compatibility promise.

The Medium findings are worth considering but are not blocking. M3 (path validation at load time) and M5 (generate_default drift risk) are the most impactful for day-to-day usability.

**Recommendation:** Address High findings, then accept.
