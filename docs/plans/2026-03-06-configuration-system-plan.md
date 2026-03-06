# Configuration System (ADR-0003) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Produce an accepted ADR-0003 for the configuration system (FR10), with design doc backing and two reviewer signoffs.

**Architecture:** Monolithic typed struct approach — `Config` hierarchy with `serde` + `garde` for YAML loading, validation, and default resolution. Walk-up-tree discovery. Secrets from env vars. No hot-reload at MVP.

**Tech Stack:** Rust, serde, serde_yml, garde, thiserror, dirs, shellexpand (all decided in ADR-0002)

---

### Task 1: Write ADR-0003

**Files:**
- Create: `docs/adrs/0003-configuration-system.md`

**Step 1: Write the ADR**

Follow the hybrid Nygard + MADR format from `AGENTS.md`. The ADR should contain:

```markdown
# ADR-0003: Configuration System

## Status
Proposed

## Context

The configuration system is the foundational layer that every subsystem reads from: the lifecycle engine (ADR-0001), CLI (FR6), agent plugins (FR1), tracker plugins (FR16), and the prompt system (FR11). It must load, validate, and provide typed access to `agent-orchestrator.yaml`.

Three constraints shape the design:

1. **Every downstream FR depends on config.** CLI commands (`ao init`, `ao start`, `ao spawn`) read project definitions. Agent plugins read `agentConfig`. Tracker plugins read `tracker.plugin` and state mappings. The config schema is the contract between subsystems.
2. **The PRD specifies 42 config options** across top-level and per-project scopes (FR10, Section 4). MVP needs a subset; the schema must accept but ignore post-MVP fields so early adopters' config files don't break as features land.
3. **ADR-0002 locked in the tech stack**: `serde` + `serde_yml` for deserialization, `garde` for validation, Rust structs for type safety. The question is how to structure the schema, discovery, validation, and public API — not which libraries to use.

The PRD requires hot-reload (watch file, reject invalid changes, keep last-known-good config). This is deferred to post-MVP; the system loads config once at startup.

## Considered Options

1. **Monolithic Typed Struct** — A single `Config` struct hierarchy with all fields known at compile time. `serde(default)` for optional fields, `garde::Validate` for semantic validation. Post-MVP fields stored as `Option<T>` or `Option<Value>`. Plugin-specific config uses `#[serde(flatten)]` for forward-compatibility.

2. **Layered Config with Explicit Merge** — Separate partial-config structs (`PartialConfig` with all fields `Option<T>`) for each source layer: built-in defaults, home directory config, project config, env var overrides, CLI flag overrides. A `merge()` function combines layers bottom-up into a resolved `Config`. Enables multi-file config splitting.

3. **Schema-Driven with Plugin Extension Points** — Core config struct handles orchestrator-owned fields. Each plugin slot registers additional config schemas at runtime via a trait method returning JSON Schema. Plugin-specific config lives in `serde_json::Value` blobs, validated by the plugin at load time.

## Decision

Option 1: Monolithic Typed Struct.

**Why not Option 2 (Layered Merge)?** It requires two struct variants per config level (partial + resolved) and merge logic for every field. Multi-file config splitting provides little MVP value — a single YAML file is sufficient. Layered merging can be added later without schema changes by introducing a `PartialConfig` variant that deserializes into the existing `Config` via a builder.

**Why not Option 3 (Schema-Driven)?** It loses compile-time type safety for plugin config, requires a JSON Schema validator dependency, and is over-engineered when we have ~3 plugins at MVP. ADR-0002 chose Rust specifically for exhaustive matching and explicit traits; dynamic schema validation works against that choice.

**Why Option 1?** It plays to Rust's strengths: the compiler enforces that every config access is type-checked. `serde(default)` handles optional fields cleanly. `#[serde(flatten)]` on `AgentConfig.extra` provides a forward-compatibility seam — core validates known fields, agent plugins validate their own extras when FR1 lands. Post-MVP fields use `Option<Value>` and deserialize without error, so users can add them to config files early without breakage.

**Key design decisions:**

### Schema

- **`camelCase` in YAML** matches PRD field names. Rust structs use `snake_case` with `#[serde(rename_all = "camelCase")]`.
- **`projects` is `HashMap<String, ProjectConfig>`** — the key is the project ID used throughout the system.
- **Per-project plugin fields (`runtime`, `agent`, `workspace`) are `Option<String>`** — `None` inherits from `defaults`.
- **`AgentConfig.extra: HashMap<String, Value>`** via `#[serde(flatten)]` absorbs agent-specific fields without schema changes.
- **Post-MVP fields** (`notifiers`, `notificationRouting`, `reactions`) are `Option<Value>` — accepted but not validated until their FRs land.

### Config Discovery

Follows PRD Section 4 (FR10) search order:
1. `AO_CONFIG_PATH` environment variable (direct path, error if missing)
2. Walk up directory tree from CWD for `agent-orchestrator.yaml` / `.yml`
3. Home directory fallback: `~/.agent-orchestrator.yaml`, `~/.agent-orchestrator.yml`, `~/.config/agent-orchestrator/config.yaml`

### Loading Pipeline

```
discover_config_path()
  → read file to string
  → serde_yml::from_str::<Config>()       // Pass 1: structural
  → config.resolve_defaults()             // inherit defaults into projects
  → config.validate(&())                  // Pass 2: semantic (garde)
  → Ok(Config)
```

### Validation (Two-Pass)

**Pass 1 — Structural:** `serde` deserialization catches missing required fields, wrong types, malformed YAML.

**Pass 2 — Semantic:** `garde` + custom validators for cross-field constraints:
- `projects` non-empty (`garde(length(min = 1))`)
- `repo` matches `"owner/repo"` pattern (`garde(pattern(...))`)
- `path` exists on disk (custom validator with tilde expansion)
- `permissions` ∈ `{"skip", "default"}`, `sandbox` ∈ `{"workspace-write", "read-only", "full"}`
- Plugin names are known values (runtime, agent, workspace, tracker, scm)
- `agent_rules` and `agent_rules_file` are mutually exclusive per project

### Secrets

Environment-variable secrets (`LINEAR_API_KEY`, `SLACK_WEBHOOK_URL`, `COMPOSIO_API_KEY`) live in a separate `ResolvedSecrets` struct with no `Serialize` derive, preventing accidental logging or disk writes.

### Public API

Four functions in `packages/core/src/config/mod.rs`:
- `load() -> Result<Config, ConfigError>` — discover + load + validate
- `load_from_path(path) -> Result<Config, ConfigError>` — load from explicit path
- `load_secrets() -> ResolvedSecrets` — env var secrets
- `generate_default(project_id, repo, path) -> String` — default YAML for `ao init`

### Module Structure

```
packages/core/src/config/
├── mod.rs          # Public API
├── schema.rs       # Config, ProjectConfig, Defaults, etc.
├── discovery.rs    # discover_config_path()
├── validation.rs   # Custom validators
├── secrets.rs      # ResolvedSecrets
└── error.rs        # ConfigError
```

### Ownership Model

`Config` is loaded once, shared via `&Config` or `Arc<Config>`. No interior mutability at MVP. Upgrade path: `ArcSwap<Config>` for hot-reload post-MVP.

### Deferred to Post-MVP

| Feature | Deferred To | Reason |
|---------|-------------|--------|
| Hot-reload | Post-MVP | Restart to pick up changes |
| Multi-file layering | Post-MVP | Single file sufficient |
| `notifiers` / `reactions` validation | FR4 / FR12 | `Option<Value>` until then |
| `maxConcurrentAgentsByState` | FR5 | `Option<Value>` until then |
| Config linting (`ao config check`) | FR6 | CLI surface |

## Consequences

**Positive:**
- Compile-time type safety for all config access — misspelled field names are caught at build time, not at runtime.
- Two-pass validation gives clear, actionable error messages with file path and field path.
- `serde(default)` + `Option<Value>` for post-MVP fields means config files are forward-compatible — users can add fields before their FRs land without breaking validation.
- `#[serde(flatten)]` on `AgentConfig.extra` lets agent plugins extend config without core schema changes.
- Small public API (4 functions) keeps the surface area minimal for downstream consumers.
- Walk-up-tree discovery follows established conventions (git, npm, cargo) — users don't need to learn a new pattern.

**Negative:**
- Adding new validated config fields requires recompiling — but that's expected and desirable in Rust (the compiler catches misuse of new fields).
- Plugin-specific config validation is deferred to FR1 — until then, `AgentConfig.extra` accepts any key/value without validation.
- No hot-reload at MVP means config changes require an orchestrator restart.
- Single-file config means users can't split global defaults from project-specific overrides into separate files.
```

**Step 2: Commit**

```bash
git add docs/adrs/0003-configuration-system.md
git commit -m "docs(adr): add ADR-0003 configuration system (Proposed)

Monolithic typed struct approach with serde + garde. Covers schema,
walk-up-tree discovery, two-pass validation, and public API. Defers
hot-reload and multi-file layering to post-MVP.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Update ADR README Index

**Files:**
- Modify: `docs/adrs/README.md:17-21`

**Step 1: Add ADR-0003 to the index table**

Add the following row after the ADR-0002 row:

```markdown
| [ADR-0003](0003-configuration-system.md) | Configuration System | Proposed |
```

**Step 2: Update the Layered Approach section**

The "Layer 2 (future)" note at line 27 mentions config validation library as a future decision. Update to reflect that it's now decided:

```markdown
## Layered Approach

ADRs are organized in layers. This first layer contains **foundational** decisions that gate downstream choices:

- **Layer 1 (this set):** Core architecture, isolation strategy, runtime, lifecycle, reactions, persistence, implementation language, and configuration system.
- **Layer 2 (future):** CLI framework, dashboard framework, mobile framework, test framework, real-time transport, monorepo structure. These depend on the implementation language decision (ADR-0002).
```

**Step 3: Commit**

```bash
git add docs/adrs/README.md
git commit -m "docs(adr): add ADR-0003 to index, update layered approach

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3: Dispatch Parallel Reviews (Gemini + Codex)

**Files:**
- Create: `docs/adrs/0003-configuration-system-review-1-gemini.md`
- Create: `docs/adrs/0003-configuration-system-review-1-codex.md`

**Step 1: Dispatch two reviewers in parallel**

Launch two subagents simultaneously, each with instructions to:

1. Read the ADR at `docs/adrs/0003-configuration-system.md`
2. Read the design doc at `docs/plans/2026-03-06-configuration-system-design.md`
3. Read the PRD FR10 section at `docs/prds/0001-agent-orchestrator.md` (lines 185-258)
4. Read ADR-0001 and ADR-0002 for context
5. Review per AGENTS.md guidelines: categorize findings as Critical/High/Medium/Low, cite evidence, separate strengths from gaps, prioritize actionable recommendations

**Gemini reviewer prompt:**
```
You are reviewing ADR-0003: Configuration System. Review the ADR and its backing design doc against the PRD requirements (FR10). Focus on:
- Schema completeness: does the ADR cover all 42 PRD config options (or explicitly defer them)?
- Discovery order: does walk-up-tree match PRD Section 4?
- Validation: are the two-pass semantics sound? Any edge cases missed?
- API surface: is the public API sufficient for downstream consumers (CLI, lifecycle engine, plugins)?
- Consequences: are there missing negative consequences?
Categorize findings as Critical/High/Medium/Low. Save your review to docs/adrs/0003-configuration-system-review-1-gemini.md.
```

**Codex reviewer prompt:**
```
You are reviewing ADR-0003: Configuration System. Review the ADR and its backing design doc against the PRD requirements (FR10). Focus on:
- Rust-specific concerns: are serde + garde the right choice? Any crate-level risks?
- Forward-compatibility: does the Option<Value> + flatten strategy hold up as FRs land?
- Error handling: is ConfigError sufficient? Any error paths missing?
- Module structure: does the layout in packages/core/src/config/ make sense for Cargo workspaces?
- Integration with ADR-0001 and ADR-0002: any conflicts or gaps?
Categorize findings as Critical/High/Medium/Low. Save your review to docs/adrs/0003-configuration-system-review-1-codex.md.
```

**Step 2: Commit reviews**

```bash
git add docs/adrs/0003-configuration-system-review-1-gemini.md docs/adrs/0003-configuration-system-review-1-codex.md
git commit -m "docs(adr): add round 1 reviews for ADR-0003 (Gemini + Codex)

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 4: Address Review Findings

**Files:**
- Modify: `docs/adrs/0003-configuration-system.md`
- Possibly modify: `docs/plans/2026-03-06-configuration-system-design.md`

**Step 1: Read both reviews**

Read `docs/adrs/0003-configuration-system-review-1-gemini.md` and `docs/adrs/0003-configuration-system-review-1-codex.md`.

**Step 2: Triage findings**

- **Critical:** Must fix before accepting. These are showstoppers.
- **High:** Should fix. Address all High findings unless there's a strong reason to defer.
- **Medium/Low:** Document as acknowledged. Fix if easy, otherwise note in ADR consequences.

**Step 3: Update ADR with fixes**

Apply changes to the ADR. For each High+ finding, either:
- Fix it in the ADR text
- Add it to the Consequences section as an acknowledged trade-off
- Add it to the Deferred table with reasoning

**Step 4: Commit**

```bash
git add docs/adrs/0003-configuration-system.md
git commit -m "docs(adr): address round 1 review findings for ADR-0003

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 5: Accept ADR-0003

**Files:**
- Modify: `docs/adrs/0003-configuration-system.md` (line 4: change `Proposed` to `Accepted`)
- Modify: `docs/adrs/README.md` (update status in index table)

**Step 1: Update ADR status to Accepted**

Change `## Status` section from `Proposed` to `Accepted`.

**Step 2: Update README index**

Change the ADR-0003 row status from `Proposed` to `Accepted`.

**Step 3: Commit**

```bash
git add docs/adrs/0003-configuration-system.md docs/adrs/README.md
git commit -m "docs(adr): accept ADR-0003 configuration system

Reviews passed (Gemini + Codex round 1). High findings addressed.
Monolithic typed struct with serde + garde for config loading,
validation, and default resolution.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 6: Update Memory

**Files:**
- Modify: `/Users/geraldyeo/.claude/projects/-Users-geraldyeo-Code-after6ix-conductor/memory/MEMORY.md`

**Step 1: Add ADR-0003 entry**

Add under `## ADR Status`:

```markdown
- ADR-0003: Configuration System — **Accepted** (monolithic typed struct)
  - Design doc: `docs/plans/2026-03-06-configuration-system-design.md`
  - Key decisions: serde + garde, walk-up-tree discovery, two-pass validation, `#[serde(flatten)]` for agent config forward-compat
  - Deferred: hot-reload, multi-file layering, post-MVP field validation
  - Reviewed by Gemini + Codex round 1
```

**Step 2: Update MVP Critical Path**

Update the note about remaining FRs:
```markdown
## Next ADR Candidates
- FR1 (Agent Plugin Contract) — needs config schema (now decided in ADR-0003)
- FR16 (Tracker Integration) — needs config for tracker.plugin
- FR6 (CLI) — needs config + agent + tracker
- FR11 (Prompt System) — needs agent + tracker for context
```
