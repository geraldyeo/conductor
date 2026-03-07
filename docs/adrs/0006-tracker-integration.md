# ADR-0006: Tracker Integration

## Status
Accepted

## Context

The orchestrator must bridge external tracker state (GitHub Issues at MVP, Linear post-MVP) with the session lifecycle engine. Every poll tick, the engine needs to know whether a session's linked issue is still active or has reached a terminal state. This `trackerState` input drives the `cleanup` global edge at precedence 28 (ADR-0001) — the mechanism that kills agents and reclaims workspaces when issues are closed, canceled, or otherwise resolved.

Five prior ADRs constrain the design:

1. **ADR-0001 (Session Lifecycle Engine)** defines `PollContext.trackerState` as a gather-phase input, consumed by the global edge `defineGlobalEdge("cleanup", 28, ctx => ctx.trackerState === "terminal")`. The gather phase is sequential per session: runtime → activity → PR/CI → tracker.
2. **ADR-0002 (Implementation Language)** locks in `CommandRunner` for subprocess management, `async-trait` for async traits, and `tokio` for the async runtime.
3. **ADR-0003 (Configuration System)** defines `TrackerConfig { plugin, team_id, active_states, terminal_states }` in `ProjectConfig`. State classification is config-driven.
4. **ADR-0004 (Plugin System)** defines the static factory pattern (`match` dispatch), `PluginMeta`, and the declarative plan pattern.
5. **ADR-0005 (Workspace & Session Metadata)** defines `SessionMetadata.issue_id` (set at creation, never changes), `SessionStore::list()` for recovery, and `TerminationReason::TrackerTerminal`.

Key forces:

- The PRD (FR16) specifies 9 methods on the Tracker interface. Not all are needed at MVP — `list_issues` requires FR5 (Scheduling), `update_issue` and label management require FR4 (Reactions).
- The PRD lists `generatePrompt()` on the Tracker, but prompt composition is a cross-cutting concern belonging to ADR-0008 (Prompt System). The Tracker should provide raw issue content, not formatted prompts.
- GitHub Issues have only two states (`open`, `closed`). Linear has many (`Backlog`, `Todo`, `In Progress`, `Done`, `Canceled`, etc.). The design must handle both through config-driven state mapping.
- The `gh` CLI (per CLAUDE.md tech stack) handles auth, rate limiting, token refresh, and Enterprise Server URLs. Replicating this in Rust adds complexity without MVP benefit.
- Issue content (body, comments) may contain adversarial content that could manipulate agents. Sanitization is a prompt system concern, not a tracker concern.

## Considered Options

### Implementation Strategy

1. **Thin trait + `gh` CLI direct** — The Tracker trait is a thin async interface. The GitHub implementation shells out to `gh` CLI via `CommandRunner` for issue fetches and comments, using `gh --json` for structured output. State classification is a pure function over `TrackerConfig`.

2. **Direct HTTP client** — Use `reqwest` to call GitHub REST/GraphQL APIs directly. Manage auth tokens, rate limit headers, and pagination in Rust. Full control over request batching.

3. **Hybrid — `gh` for auth, HTTP for queries** — Use `gh auth token` to extract credentials, then make HTTP requests via `reqwest`. Leverages `gh`'s credential management without per-call subprocess overhead.

### Prompt Generation

4. **`generatePrompt()` on Tracker** — Tracker returns a formatted prompt string from issue data. Self-contained but couples prompt formatting to the tracker plugin.

5. **Structured `IssueContent` return** — Tracker returns `IssueContent { title, body, comments }` as raw data. The prompt system (ADR-0008) handles formatting and injection sanitization.

### Unmatched State Handling

6. **Unmatched defaults to active** — Issue states not in `active_states` or `terminal_states` are treated as active. Conservative — keeps sessions alive.

7. **Third `unknown` value** — Introduce `TrackerState::Unknown` that the lifecycle engine ignores. Requires PollContext schema change.

### Recovery Scope

8. **Session-scoped recovery** — On restart, re-poll issues for known non-terminal sessions only. No tracker scanning for new issues.

9. **Full tracker scan** — Also call `list_issues()` to discover issues that entered active states during downtime and auto-spawn sessions.

## Decision

**Implementation:** Option 1 — thin trait + `gh` CLI direct. Subprocess overhead is negligible at MVP scale (one call per session per 30s tick). `gh` handles auth complexity. The trait design is implementation-independent — swapping to Option 3 post-MVP changes only the GitHub module.

**Prompt generation:** Option 5 — structured `IssueContent`. The Tracker returns raw data; ADR-0008 (Prompt System) handles composition and injection sanitization.

**Unmatched states:** Option 6 — default to active. Keeps sessions alive conservatively. The next poll tick corrects if the state changes.

**Recovery:** Option 8 — session-scoped only. Full tracker scanning requires `list_issues()` and FR5 scheduling logic, both deferred.

**The design has five components:**

### 1. Tracker Trait & Types

The trait follows ADR-0004's patterns: `meta()` for plugin metadata, `async_trait`, `Send + Sync`, static factory.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub id: String,              // "42" (GitHub) or "ABC-123" (Linear)
    pub state: String,           // raw tracker state: "open", "closed", "In Progress", etc.
    pub title: String,
    pub url: String,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IssueContent {
    pub title: String,
    pub body: String,
    pub comments: Vec<IssueComment>,
}

/// Timestamps use ISO 8601 format (e.g., "2026-03-07T10:00:00Z").
/// String type chosen for simplicity — the Tracker returns whatever the
/// external API provides. Downstream consumers parse as needed.
#[derive(Debug, Clone)]
pub struct IssueComment {
    pub author: String,
    pub body: String,
    pub created_at: String,  // ISO 8601
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackerState {
    Active,
    Terminal,
}

#[async_trait]
pub trait Tracker: Send + Sync {
    fn meta(&self) -> PluginMeta;

    // --- MVP (required) ---
    async fn get_issue(&self, issue_id: &str) -> Result<Issue, TrackerError>;
    fn branch_name(&self, issue_id: &str, title: &str) -> String;
    fn issue_url(&self, issue_id: &str) -> String;
    async fn get_issue_content(&self, issue_id: &str) -> Result<IssueContent, TrackerError>;
    async fn add_comment(&self, issue_id: &str, body: &str) -> Result<(), TrackerError>;

    // --- Post-MVP (default to NotImplemented) ---
    async fn list_issues(&self) -> Result<Vec<Issue>, TrackerError> { .. }
    async fn update_issue(&self, issue_id: &str, update: &IssueUpdate) -> Result<(), TrackerError> { .. }
    async fn create_issue(&self, create: &IssueCreate) -> Result<Issue, TrackerError> { .. }
    async fn add_label(&self, issue_id: &str, label: &str) -> Result<(), TrackerError> { .. }
    async fn remove_label(&self, issue_id: &str, label: &str) -> Result<(), TrackerError> { .. }
}
```

**State classification** is a pure function outside the trait — it's orchestrator policy (configured in YAML), not tracker logic:

```rust
pub fn classify_state(issue_state: &str, config: &TrackerConfig) -> TrackerState {
    if config.terminal_states.iter().any(|s| s.eq_ignore_ascii_case(issue_state)) {
        TrackerState::Terminal
    } else {
        TrackerState::Active
    }
}
```

Case-insensitive comparison accommodates trackers that report states in varying casing.

**`active_states` scope:** `classify_state()` only checks `terminal_states`. The `active_states` config field is not consumed at MVP — it exists for FR5 (Scheduling), where the scheduler uses it to filter which issues are eligible for auto-dispatch. At MVP, only manual `ao spawn` creates sessions, so `active_states` has no consumer. This is documented here to avoid confusion: users may configure `activeStates` in YAML, but it has no effect until FR5 lands.

**Factory registration and startup validation:**

```rust
pub fn create_tracker(name: &str, repo: &str, config: &TrackerConfig)
    -> Result<Box<dyn Tracker>, PluginError>
{
    match name {
        "github" => {
            let tracker = GitHubTracker::new(repo, config);
            tracker.validate()?;  // fail-fast: checks gh presence + auth
            Ok(Box::new(tracker))
        }
        "linear" => Err(PluginError::NotImplemented("linear".into())),
        _ => Err(PluginError::UnknownPlugin(name.into())),
    }
}
```

`GitHubTracker::validate()` runs `gh auth status` at construction time and returns an error if `gh` is not installed or not authenticated. This follows ADR-0004's pattern: "Startup validation checks all plugin names referenced in config are known, before any sessions are created." Discovering `gh` is missing on the first poll tick is too late — fail fast at startup.
```

**Error types** (via `thiserror`):

```rust
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("issue not found: {0}")]
    NotFound(String),
    #[error("rate limited, retry after {0:?}")]
    RateLimited(Duration),
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("failed to parse tracker response: {0}")]
    ParseError(String),
    #[error("not implemented: {0}")]
    NotImplemented(String),
}
```

### 2. GitHub Implementation

`GitHubTracker` shells out to `gh` CLI via `CommandRunner`. All `gh` calls use `--json` for structured output parsed via `serde_json`.

```rust
pub struct GitHubTracker {
    repo: String,           // "owner/repo" from ProjectConfig
    config: TrackerConfig,
}
```

**Method-to-CLI mapping:**

| Method | Command | Notes |
|--------|---------|-------|
| `get_issue()` | `gh issue view {id} --repo {repo} --json number,state,title,url,assignees,labels` | JSON → `Issue` |
| `get_issue_content()` | `gh issue view {id} --repo {repo} --json title,body,comments` | JSON → `IssueContent` |
| `add_comment()` | `gh issue comment {id} --repo {repo} --body {body}` | Exit code only |
| `branch_name()` | Pure: `{issue_id}-{slugified_title}` | No CLI call |
| `issue_url()` | Pure: `https://github.com/{repo}/issues/{issue_id}` | No CLI call |

**Branch name generation:** slugifies the title (lowercase, non-alphanumeric → hyphens), truncates to 50 characters, prepends issue ID: `42-fix-login-bug`. The slug is ASCII-only after the mapping step (all non-alphanumeric characters including multi-byte UTF-8 are replaced with hyphens), so byte-index truncation is safe. If the title is empty or produces an empty slug after filtering, the branch name falls back to the issue ID only (e.g., `42`).

**Input validation:** GitHub implementation validates `issue_id` as a positive integer before shelling out. `CommandRunner` passes arguments as `Vec<String>` (not shell-interpolated) for defense in depth against command injection.

**Rate limiting:** `gh` CLI handles rate limiting internally (retries with backoff). If it still fails, `TrackerError::RateLimited` propagates. The gather phase defaults to `TrackerState::Active` on any error — convergence corrects on the next tick.

### 3. Gather Phase Integration

Tracker is step 4 of 4 in the per-session gather sequence (ADR-0001):

```
1. Runtime liveness  → runtimeAlive
2. Activity state    → activityState
3. PR/CI/review      → pr { ... }
4. Tracker state     → trackerState
```

The gatherer calls `get_issue()` and `classify_state()`:

```rust
match tracker.get_issue(&session.issue_id).await {
    Ok(issue) => {
        not_found_counts.remove(&session.id);
        ctx.tracker_state = classify_state(&issue.state, &tracker_config);
    }
    Err(TrackerError::NotFound(_)) => {
        let count = not_found_counts.entry(session.id.clone()).or_insert(0);
        *count += 1;
        if *count >= 2 {
            ctx.tracker_state = TrackerState::Terminal;  // confirmed deletion → terminal
        } else {
            ctx.tracker_state = TrackerState::Active;    // first NotFound → grace period
            tracing::warn!(
                "Issue {} not found — may have been deleted. \
                 Will treat as terminal on next tick if still not found.",
                session.issue_id
            );
        }
    }
    Err(_) => {
        ctx.tracker_state = TrackerState::Active;    // API failure → safe default
    }
}
```

Deleted issues (`NotFound`) use a **two-tick grace period** before being treated as terminal. The first `NotFound` defaults to `Active` (same safe default as other errors) and logs a warning. The second consecutive `NotFound` confirms the deletion and sets `Terminal`. This gives humans one poll cycle (~30s) to react to accidental issue deletions before the `cleanup` global edge fires. The `not_found_counts` map is held in-memory per gatherer instance (not persisted) — on restart it resets to zero, which is the safe direction (requires two fresh ticks to confirm). A successful `get_issue()` clears the counter.

API failures default to active — a transient error shouldn't kill a session.

> **Note:** Before workspace destruction, the lifecycle engine archives session metadata (ADR-0005). However, uncommitted changes in the git worktree are lost on `git worktree remove --force`. Agents are instructed (via base prompt, ADR-0008) to commit frequently, which reduces but does not eliminate this risk.

**PollContext update:** `trackerState` changes from a string literal type (ADR-0001) to the `TrackerState` enum. The `TrackerState` enum lives in `packages/core/src/tracker/mod.rs`.

### 4. Session Creation Integration

The Tracker participates in session creation at three points:

**Pre-spawn validation** (before ADR-0005 step 2): Reject early if issue doesn't exist or is terminal. No resources to unwind.

```rust
let issue = tracker.get_issue(&issue_id).await?;
let state = classify_state(&issue.state, &tracker_config);
if state == TrackerState::Terminal {
    return Err(SpawnError::IssueTerminal(issue_id));
}
```

**Branch name derivation** (before ADR-0005 step 3):

```rust
let branch = tracker.branch_name(&issue_id, &issue.title);
```

**Workpad comment** (after successful creation, non-blocking):

```rust
let comment = format!(
    "**Agent session started**\n- Session: `{session_id}`\n- Agent: `{agent}`\n- Branch: `{branch}`"
);
if let Err(e) = tracker.add_comment(&issue_id, &comment).await {
    tracing::warn!("Failed to post session comment: {e}");
}
```

Comment failure is logged but does not block session creation.

### 5. Recovery on Restart

Session-scoped only. On restart:

1. `SessionStore::list()` loads non-terminal sessions.
2. The lifecycle engine runs one immediate poll tick (ADR-0001 crash recovery).
3. The gather phase calls `tracker.get_issue()` for each session's `issue_id`.
4. Sessions whose issues went terminal during downtime: `trackerState = Terminal` → `cleanup` global edge fires → runtime destroyed, workspace cleaned up.

No tracker scanning. No auto-dispatch. The existing poll loop is the reconciliation mechanism.

### Module Structure

```
packages/core/src/tracker/
├── mod.rs          # Tracker trait, TrackerState, classify_state(), factory, types
├── github.rs       # GitHubTracker implementation
└── error.rs        # TrackerError
```

### PRD Interface Mapping

| PRD Method | ADR Mapping |
|------------|-------------|
| `getIssue()` | `get_issue()` |
| `isCompleted()` | Removed — replaced by `classify_state()` pure function over `get_issue().state` |
| `issueUrl()` | `issue_url()` |
| `branchName()` | `branch_name()` |
| `generatePrompt()` | Replaced by `get_issue_content()` — raw data, not formatted prompt |
| `listIssues()` | `list_issues()` (post-MVP) |
| `updateIssue()` | `update_issue()` (post-MVP) |
| `createIssue()` | `create_issue()` (post-MVP) |
| `issueLabel()` | Split into `add_label()` / `remove_label()` (post-MVP) |

### Deferred Items

| Feature | Deferred to | Reason |
|---------|-------------|--------|
| `list_issues()` impl | FR5 (Scheduling) | Auto-dispatch requires scheduling |
| `update_issue()` impl | FR4 (Reactions) | State mutations are reaction-driven |
| `create_issue()` impl | Post-MVP | No MVP use case |
| `add_label()` / `remove_label()` | FR4 (Reactions) | Label management is reaction-driven |
| Linear tracker plugin | Post-MVP | GitHub-only at MVP |
| Batched GraphQL queries | Post-MVP | Subprocess-per-call fine at MVP scale |
| `deletedIssuePolicy` config | Post-MVP | MVP uses two-tick confirmation (grace period); post-MVP adds configurable policy (`terminal`, `warn`, `ignore`) |
| Token-scoped auth | FR17 (Mutation Authority) | MVP uses ambient `gh` auth |
| Prompt injection sanitization | ADR-0008 (Prompt System) | `IssueContent` returned verbatim |

Reference `docs/plans/2026-03-07-tracker-integration-design.md` for full pseudocode, testing strategy, and detailed rationale.

## Consequences

Positive:

- The Tracker trait is thin and focused — 5 MVP methods, all with clear single responsibilities. New tracker backends (Linear, Jira) implement the same trait without changing the lifecycle engine.
- State classification is decoupled from the tracker — `classify_state()` is a pure function over config, testable without I/O. The same function works for GitHub (2 states) and Linear (many states) because the mapping is config-driven.
- `IssueContent` separates data retrieval from prompt formatting. The Tracker fetches verbatim; the prompt system (ADR-0008) composes and sanitizes. Neither subsystem knows about the other's internals.
- `gh` CLI handles auth, rate limiting, token refresh, and Enterprise Server URLs — none of this needs to be reimplemented. `gh --json` provides stable, structured output. The trait design is implementation-independent, so upgrading to direct HTTP post-MVP changes only the GitHub module.
- Gather-phase error handling follows ADR-0001's convergence principle: deleted issues → terminal after a two-tick grace period, API failures → active (safe default), corrected on the next poll tick. No single-tick failure kills a session, and accidental issue deletions have a ~30s window for human intervention before workspace destruction.
- Pre-spawn validation rejects terminal or missing issues before creating any resources, avoiding the unwind sequence from ADR-0005 steps 2-8.
- Recovery is free — the existing poll loop handles it. No special recovery code, no tracker scanning, no additional state to manage.
- `branch_name()` on the Tracker (not the Workspace) allows tracker-specific branch naming conventions. GitHub uses `{id}-{slug}`, Linear could use its native `{team}-{number}-{slug}` format.

Negative:

- One subprocess per session per poll tick for `get_issue()`. At 30s intervals with <20 sessions (MVP scale), this is ~40 subprocess calls per minute — negligible. At higher scale, batched GraphQL queries (one call for all sessions) would reduce this to 1-2 calls per tick. Documented as a post-MVP optimization.
- `gh` CLI is an external dependency that must be installed and authenticated. If `gh` is not available, the orchestrator cannot start with the `github` tracker. The factory function validates `gh` presence and auth at construction time (fail-fast).
- `gh --json` output format is stable but not versioned. A `gh` upgrade that changes JSON field names would break parsing. Mitigated by pinning `gh` version in CI and testing against captured output.
- `branch_name()` slugification may produce collisions for issues with similar titles (e.g., "Fix bug" and "Fix bug!"). The issue ID prefix makes this unlikely in practice, and `git worktree add` would fail fast on collision.
- Case-insensitive state comparison (`eq_ignore_ascii_case`) may produce false matches for trackers with states that differ only by case. This is unlikely in practice — GitHub uses lowercase, Linear uses title case — and is the safer default over case-sensitive matching that would silently fail on casing mismatches.
- The workpad comment format is hardcoded. Post-MVP, a configurable comment template (part of ADR-0008 or a config extension) would allow teams to customize progress reporting.
- `IssueContent.comments` returns all comments, which may be large for long-lived issues. Post-MVP, a `limit` parameter or recency filter on `get_issue_content()` would address this.
