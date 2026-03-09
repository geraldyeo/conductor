use crate::plugins::tracker::{classify_state, Tracker, TrackerError, TrackerState};
use crate::plugins::workspace::Workspace;
use crate::prompt::{CommentContext, IssueContext, ProjectContext, PromptEngine};
use crate::store::{JournalEntry, JournalResult, SessionMetadata, SessionStore};
use crate::types::{SessionStatus, TerminationReason};
use crate::utils::DataPaths;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug)]
pub struct SpawnRequest {
    pub issue_url: String,
    pub project_name: String,
    pub project_path: PathBuf,
    pub session_prefix: String,
    pub terminal_states: Vec<String>,
    pub data_paths: DataPaths,
    pub templates_dir: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("issue is terminal: {0}")]
    IssueTerminal(String),
    #[error("issue not found: {0}")]
    IssueNotFound(String),
    #[error("session store error: {0}")]
    Store(#[from] crate::store::StoreError),
    #[error("workspace error: {0}")]
    Workspace(#[from] crate::plugins::workspace::WorkspaceError),
    #[error("prompt error: {0}")]
    Prompt(#[from] crate::prompt::PromptError),
    #[error("tracker error: {0}")]
    Tracker(#[from] TrackerError),
}

fn now_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

/// Execute the 8-step spawn sequence.
///
/// 1. Validate issue exists and is not terminal (Tracker)
/// 2. Create session record (SessionStore)
/// 3. Create workspace (Workspace)
/// 4. Run afterCreate hook (stubbed)
/// 5. Run beforeRun hook (stubbed)
/// 6. Render launch prompt (PromptEngine)
/// 7. Build LaunchPlan (Agent) — stubbed at this layer, done by caller
/// 8. Execute LaunchPlan (Runtime) — stubbed at this layer, done by caller
///
/// Steps 7-8 are left to the caller because the orchestrator doesn't own
/// agent/runtime creation. This function handles steps 1-6 and returns
/// the session_id + rendered prompt for the caller to drive steps 7-8.
pub async fn spawn_session(
    req: &SpawnRequest,
    tracker: &dyn Tracker,
    store: &SessionStore,
    workspace: &dyn Workspace,
) -> Result<String, SpawnError> {
    // Step 1: Validate issue exists and is not terminal
    let issue_content = match tracker.get_issue_content(&req.issue_url).await {
        Ok(content) => content,
        Err(TrackerError::NotFound(_)) => {
            return Err(SpawnError::IssueNotFound(req.issue_url.clone()));
        }
        Err(e) => return Err(SpawnError::Tracker(e)),
    };

    let state = classify_state(&issue_content.state, &req.terminal_states);
    if state == TrackerState::Terminal {
        return Err(SpawnError::IssueTerminal(req.issue_url.clone()));
    }

    // Derive session ID and branch
    let session_id = format!(
        "{}-{}-1",
        req.session_prefix, issue_content.number
    );
    let branch = tracker.branch_name(issue_content.number, &issue_content.title);
    let worktree_path = req.data_paths.worktree_path(&session_id);

    // Step 2: Create session record
    let now = now_timestamp();
    let initial_meta = SessionMetadata {
        session_id: session_id.clone(),
        status: SessionStatus::Spawning,
        created_at: now.clone(),
        updated_at: now,
        workspace_path: worktree_path.clone(),
        agent: "claude-code".to_string(),
        runtime: "tmux".to_string(),
        issue_id: issue_content.number.to_string(),
        attempt: 1,
        branch: branch.clone(),
        base_branch: "main".to_string(),
        pr_url: String::new(),
        tokens_in: 0,
        tokens_out: 0,
        termination_reason: None,
        kill_requested: false,
        tracker_cleanup_requested: false,
    };
    store.create(&initial_meta).await?;

    // Steps 3-6 with unwind on failure
    match spawn_inner(req, tracker, store, workspace, &session_id, &branch, &issue_content).await {
        Ok(()) => Ok(session_id),
        Err(e) => {
            tracing::error!(session_id = %session_id, error = %e, "spawn failed, unwinding");
            // Best-effort cleanup: destroy workspace
            if let Err(ws_err) = workspace.destroy(&session_id, true).await {
                tracing::warn!(session_id = %session_id, error = %ws_err, "workspace cleanup failed");
            }
            // Mark session as errored
            if let Ok(mut meta) = store.read(&session_id).await {
                meta.status = SessionStatus::Errored;
                meta.termination_reason = Some(TerminationReason::SpawnFailed);
                meta.updated_at = now_timestamp();
                let _ = store.write(&session_id, &meta).await;
            }
            Err(e)
        }
    }
}

async fn spawn_inner(
    req: &SpawnRequest,
    tracker: &dyn Tracker,
    store: &SessionStore,
    workspace: &dyn Workspace,
    session_id: &str,
    branch: &str,
    issue_content: &crate::plugins::tracker::IssueContent,
) -> Result<(), SpawnError> {
    // Step 3: Create workspace
    let ws_info = workspace.create(session_id, branch).await?;

    // Step 4: afterCreate hook (stubbed)
    // Step 5: beforeRun hook (stubbed)

    // Step 6: Render launch prompt
    let templates_dir = req.templates_dir.as_deref().unwrap_or_else(|| {
        Path::new("packages/core/templates")
    });
    let engine = PromptEngine::new(templates_dir)?;

    let issue_ctx = IssueContext {
        title: issue_content.title.clone(),
        body: issue_content.body.clone(),
        comments: issue_content
            .comments
            .iter()
            .map(|c| CommentContext {
                author: c.author.clone(),
                body: c.body.clone(),
                created_at: c.created_at.clone(),
            })
            .collect(),
        state: issue_content.state.clone(),
        labels: issue_content.labels.clone(),
        issue_url: tracker.issue_url(issue_content.number),
        number: issue_content.number,
    };
    let project_ctx = ProjectContext {
        path: req.project_path.clone(),
        name: req.project_name.clone(),
    };
    let _prompt = engine.render_launch(&issue_ctx, &project_ctx, None).await?;

    // Steps 7-8: Build LaunchPlan + Execute via Runtime
    // At MVP, these are driven by the caller (CLI layer) after spawn_session returns.
    // The prompt is rendered and the session/workspace are ready.

    // Transition status: Spawning → Working
    let mut meta = store.read(session_id).await?;
    meta.status = SessionStatus::Working;
    meta.workspace_path = ws_info.path;
    meta.updated_at = now_timestamp();
    store.write(session_id, &meta).await?;

    // Write journal entry
    let journal_entry = JournalEntry {
        action: "spawn".to_string(),
        target: session_id.to_string(),
        timestamp: now_timestamp(),
        dedupe_key: format!("spawn:{}:1", session_id),
        result: JournalResult::Success,
        error_code: None,
        attempt: 1,
        actor: "orchestrator".to_string(),
    };
    store.append_journal(session_id, &journal_entry).await?;

    Ok(())
}
