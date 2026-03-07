# Tracker Integration Design

Reference design for ADR-0006: Tracker Integration.

## 1. Problem

The orchestrator must read issue state from external trackers (GitHub Issues at MVP, Linear post-MVP) and feed it into the lifecycle engine's poll loop. This is the external-state layer — the bridge between tracker-managed issue lifecycle and orchestrator-managed session lifecycle.

Key requirements:
- Fetch issue state each poll tick and classify it as `active` or `terminal` for `PollContext.trackerState` (ADR-0001).
- Provide issue content for prompt composition (ADR-0008, future).
- Post progress comments to issues (workpad pattern).
- Derive branch names from issue metadata for workspace creation (ADR-0005).
- Recover gracefully on restart by re-polling known sessions' issues.

## 2. Design Decisions

### D1: Structured issue content, not formatted prompts

The PRD lists `generatePrompt()` on the Tracker interface. This ADR replaces it with `get_issue_content()` returning structured `IssueContent { title, body, comments }`. The prompt system (ADR-0008) composes and sanitizes this data — the Tracker returns it verbatim.

**Rationale:** The Tracker knows how to fetch data, not how to format prompts. Prompt injection cleaning (issue body may contain adversarial content) is a prompt system concern, not a tracker concern.

### D2: Unmatched states default to active

`classify_state()` checks `terminal_states` first. If the issue's raw state matches neither `active_states` nor `terminal_states`, it returns `TrackerState::Active`.

**Rationale:** Conservative — keeps sessions alive. Worst case: a session stays running one extra poll tick. The alternative (a third `Unknown` value) adds complexity for an edge case that's corrected on the next tick.

### D3: MVP trait includes 5 methods

| Method | MVP justification |
|--------|-------------------|
| `get_issue()` | Gather phase every tick |
| `branch_name()` | Workspace creation |
| `issue_url()` | CLI display, metadata |
| `get_issue_content()` | Prompt composition (ADR-0008) |
| `add_comment()` | Workpad pattern, progress updates |

Post-MVP methods (`list_issues`, `update_issue`, `create_issue`, `add_label`, `remove_label`) are declared on the trait with default `NotImplemented` returns.

### D4: Session-scoped recovery only

On restart, the orchestrator loads non-terminal sessions from `SessionStore::list()` and re-polls their issues. No tracker scanning for new active issues — that requires `list_issues()` and FR5 scheduling.

### D5: `gh` CLI via CommandRunner

The GitHub implementation shells out to `gh` CLI using `CommandRunner` (ADR-0002). `gh --json` returns structured, parseable output. `gh` handles auth, token refresh, rate limiting, and Enterprise Server URLs.

**Rationale:** Don't reinvent the wheel. Subprocess overhead is negligible at MVP scale (one call per session per 30s tick). Upgrade path to direct HTTP (Approach 3) is a GitHub module refactor, not a trait change.

## 3. Tracker Trait & Types

```rust
/// Issue data returned by the tracker. Fields are tracker-agnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub id: String,              // "42" (GitHub) or "ABC-123" (Linear)
    pub state: String,           // raw tracker state: "open", "closed", "In Progress", etc.
    pub title: String,
    pub url: String,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
}

/// Structured issue content for prompt composition (ADR-0008).
/// Returned verbatim — no sanitization. Prompt system handles injection cleaning.
#[derive(Debug, Clone)]
pub struct IssueContent {
    pub title: String,
    pub body: String,
    pub comments: Vec<IssueComment>,
}

/// Timestamps use ISO 8601 format (e.g., "2026-03-07T10:00:00Z").
#[derive(Debug, Clone)]
pub struct IssueComment {
    pub author: String,
    pub body: String,
    pub created_at: String,  // ISO 8601
}

/// Post-MVP: used by update_issue()
#[derive(Debug)]
pub struct IssueUpdate {
    pub state: Option<String>,
    pub title: Option<String>,
    pub assignees: Option<Vec<String>>,
}

/// Post-MVP: used by create_issue()
#[derive(Debug)]
pub struct IssueCreate {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
}

/// Resolved tracker state for PollContext. Pure enum, no tracker-specific values.
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
    async fn list_issues(&self) -> Result<Vec<Issue>, TrackerError> {
        Err(TrackerError::NotImplemented("list_issues".into()))
    }
    async fn update_issue(&self, issue_id: &str, update: &IssueUpdate) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented("update_issue".into()))
    }
    async fn create_issue(&self, create: &IssueCreate) -> Result<Issue, TrackerError> {
        Err(TrackerError::NotImplemented("create_issue".into()))
    }
    async fn add_label(&self, issue_id: &str, label: &str) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented("add_label".into()))
    }
    async fn remove_label(&self, issue_id: &str, label: &str) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented("remove_label".into()))
    }
}
```

### State Classification

A pure function, not on the trait — state classification is orchestrator policy, not tracker logic:

```rust
pub fn classify_state(
    issue_state: &str,
    config: &TrackerConfig,
) -> TrackerState {
    if config.terminal_states.iter().any(|s| s.eq_ignore_ascii_case(issue_state)) {
        TrackerState::Terminal
    } else {
        TrackerState::Active  // unmatched defaults to active
    }
}
```

### Factory Registration

```rust
pub fn create_tracker(
    name: &str,
    repo: &str,
    config: &TrackerConfig,
) -> Result<Box<dyn Tracker>, PluginError> {
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

`GitHubTracker::validate()` runs `gh auth status` via `CommandRunner` and returns `PluginError` if `gh` is not installed or not authenticated. This follows ADR-0004's startup validation pattern.

**`active_states` scope:** `classify_state()` only checks `terminal_states`. The `active_states` config field is consumed by FR5 (Scheduling) for auto-dispatch filtering — it is not used at MVP. Users may configure it in YAML, but it has no effect until FR5 lands.

### Error Types

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

## 4. GitHub Implementation

```rust
pub struct GitHubTracker {
    repo: String,           // "owner/repo" from ProjectConfig
    config: TrackerConfig,
}

impl GitHubTracker {
    pub fn new(repo: &str, config: &TrackerConfig) -> Self;
}
```

### Method-to-CLI Mapping

| Method | Command | Output |
|--------|---------|--------|
| `get_issue()` | `gh issue view {id} --repo {repo} --json number,state,title,url,assignees,labels` | JSON → `Issue` |
| `get_issue_content()` | `gh issue view {id} --repo {repo} --json title,body,comments` | JSON → `IssueContent` |
| `add_comment()` | `gh issue comment {id} --repo {repo} --body {body}` | Exit code |
| `branch_name()` | Pure: `{issue_id}-{slugified_title}` | No CLI |
| `issue_url()` | Pure: `https://github.com/{repo}/issues/{issue_id}` | No CLI |

### Branch Name Generation

```rust
fn branch_name(&self, issue_id: &str, title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // Slug is ASCII-only after mapping — byte-index truncation is safe.
    let slug = slug.trim_matches('-');
    let slug = if slug.len() > 50 { &slug[..50] } else { slug };
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        issue_id.to_string()  // fallback for empty/non-alphanumeric titles
    } else {
        format!("{issue_id}-{slug}")
    }
}
```

Note: uses `is_ascii_alphanumeric()` (not `is_alphanumeric()`) to ensure the slug is pure ASCII, making byte-index truncation safe. Non-ASCII characters (e.g., CJK, emoji) are replaced with hyphens.

### Input Validation

The GitHub implementation validates `issue_id` as a positive integer before shelling out, preventing command injection. `CommandRunner` passes arguments as `Vec<String>` (not shell-interpolated) for defense in depth.

### Error Mapping

- `gh` exit code 1 with "not found" in stderr → `TrackerError::NotFound`
- `gh` exit code 4 (HTTP 429) → `TrackerError::RateLimited` (parse `Retry-After` header if available)
- `gh` exit code 4 (HTTP 401/403) → `TrackerError::AuthFailed`
- Other non-zero exit → `TrackerError::CommandFailed`
- JSON parse failure → `TrackerError::ParseError`

Rate limiting: `gh` CLI handles retries internally. If it still fails, the error propagates. The gather phase treats this as a gather failure — `trackerState` defaults to `Active` for this tick.

## 5. Gather Phase Integration

Tracker is step 4 of 4 in the per-session gather sequence:

```
1. Runtime liveness  → runtimeAlive        (cheapest, short-circuits if dead)
2. Activity state    → activityState       (agent detection)
3. PR/CI/review      → pr { ... }          (SCM plugin)
4. Tracker state     → trackerState        (Tracker plugin)
```

### Gatherer Pseudocode

```rust
let issue = tracker.get_issue(&session.issue_id).await;
match issue {
    Ok(issue) => {
        ctx.tracker_state = classify_state(&issue.state, &tracker_config);
    }
    Err(TrackerError::NotFound(_)) => {
        // Issue deleted — treat as terminal
        ctx.tracker_state = TrackerState::Terminal;
    }
    Err(_) => {
        // API failure — default to active, convergence corrects next tick
        ctx.tracker_state = TrackerState::Active;
    }
}
```

### PollContext Update

```rust
pub struct PollContext {
    pub runtime_alive: bool,
    pub activity_state: ActivityState,
    pub pr: Option<PrContext>,
    pub tracker_state: TrackerState,  // typed enum (was string in ADR-0001)
    pub budget_exceeded: bool,
    pub manual_kill: bool,
}
```

Minor change from ADR-0001: `trackerState` moves from string literal type to `TrackerState` enum.

## 6. Session Creation Integration

### Pre-Spawn Validation

Before creating any resources, validate the issue exists and is active:

```rust
let issue = tracker.get_issue(&issue_id).await
    .map_err(|e| SpawnError::TrackerFailed(e))?;

let state = classify_state(&issue.state, &tracker_config);
if state == TrackerState::Terminal {
    return Err(SpawnError::IssueTerminal(issue_id));
}
```

### Branch Name Derivation

Called before ADR-0005 step 3 (workspace creation):

```rust
let branch = tracker.branch_name(&issue_id, &issue.title);
// Passed to WorkspaceCreateContext.branch
```

### Workpad Comment

Posted after successful session creation (non-blocking):

```rust
let comment = format!(
    "**Agent session started**\n\
     - Session: `{session_id}`\n\
     - Agent: `{agent}`\n\
     - Branch: `{branch}`"
);
if let Err(e) = tracker.add_comment(&issue_id, &comment).await {
    tracing::warn!("Failed to post session comment: {e}");
}
```

## 7. Recovery on Restart

Session-scoped only — no tracker scanning:

```rust
// 1. Load non-terminal sessions
let sessions = session_store.list()?
    .into_iter()
    .filter(|s| !s.status.is_terminal())
    .collect::<Vec<_>>();

// 2. Run one immediate poll tick (ADR-0001 crash recovery).
// Gather phase calls tracker.get_issue() for each session's issue_id.
// Sessions whose issues went terminal during downtime:
//   trackerState = Terminal → cleanup global edge (precedence 28) fires.
```

## 8. Module Structure

```
packages/core/src/tracker/
├── mod.rs          # Tracker trait, TrackerState, classify_state(), factory, types
├── github.rs       # GitHubTracker implementation
└── error.rs        # TrackerError
```

## 9. Testing Strategy

| Target | Approach |
|--------|----------|
| `classify_state()` | Unit tests: matched active, matched terminal, unmatched → active, case-insensitive |
| `branch_name()` | Unit tests: special chars, long titles, empty title, numeric-only IDs |
| `GitHubTracker` methods | Unit tests with mocked `CommandRunner` output (captured `gh --json` responses) |
| `GitHubTracker` integration | CI-only tests against a real test repo with `gh` CLI |
| Gather step 4 | Unit tests: success → classify, `NotFound` → terminal, error → active fallback |
| Pre-spawn validation | Unit tests: active issue → proceed, terminal → reject, missing → reject |

## 10. Deferred Items

| Feature | Deferred to | Reason |
|---------|-------------|--------|
| `list_issues()` implementation | FR5 (Scheduling) | Auto-dispatch requires scheduling |
| `update_issue()` implementation | FR4 (Reactions) | State mutations are reaction-driven |
| `create_issue()` implementation | Post-MVP | No MVP use case |
| `add_label()` / `remove_label()` | FR4 (Reactions) | Label management is reaction-driven |
| Linear tracker plugin | Post-MVP | GitHub-only at MVP |
| Batched GraphQL queries | Post-MVP | Subprocess-per-call fine at MVP scale |
| `deletedIssuePolicy` config | Post-MVP | Hardcoded to terminal at MVP |
| Token-scoped auth (FR17) | FR17 (Mutation Authority) | MVP uses ambient `gh` auth |
| Prompt injection sanitization | ADR-0008 (Prompt System) | `IssueContent` returned verbatim |
