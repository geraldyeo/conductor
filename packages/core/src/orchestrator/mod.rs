use crate::agent::{create_agent, Agent};
use crate::config::Config;
use crate::ipc::{
    read_message, socket_path, write_message, BatchSpawnItem, BatchSpawnOutcome,
    OrchestratorRequest, OrchestratorResponse,
};
use crate::lifecycle::graph::StateGraph;
use crate::lifecycle::poll::PollTick;
use crate::plugins::tracker::{classify_state, GitHubTracker, Tracker, TrackerState};
use crate::prompt::{CommentContext, IssueContext, ProjectContext};
use crate::runtime::{create_runtime, Runtime};
use crate::session_store::{SessionMetadata, SessionStore};
use crate::utils::{CommandRunner, DataPaths};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{error, info, warn};

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("config error: {0}")]
    Config(#[from] crate::config::ConfigError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("orchestrator already running (socket exists at {0:?})")]
    AlreadyRunning(PathBuf),
    #[error("project not found: {0}")]
    ProjectNotFound(String),
    #[error("issue is in terminal state")]
    IssueTerminal,
    #[error("issue not found: {0}")]
    IssueNotFound(String),
    #[error("session already exists: {0}")]
    SessionExists(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("workspace error: {0}")]
    WorkspaceError(String),
    #[error("runtime error: {0}")]
    RuntimeError(String),
    #[error("tracker error: {0}")]
    TrackerError(String),
    #[error("data paths error: {0}")]
    DataPaths(#[from] crate::utils::DataPathsError),
    #[error("store error: {0}")]
    Store(#[from] crate::session_store::StoreError),
}

struct ProjectPlugins {
    agent: Arc<dyn Agent>,
    runtime: Arc<dyn Runtime>,
    store: Arc<SessionStore>,
    terminal_states: Vec<String>,
    repo_root: PathBuf,
}

type IpcMsg = (OrchestratorRequest, oneshot::Sender<OrchestratorResponse>);

pub struct Orchestrator {
    config: Arc<Config>,
    plugins: HashMap<String, ProjectPlugins>,
    graph: Arc<StateGraph>,
    socket_path: PathBuf,
    /// Signals the run loop to exit. Stored as a field so `handle_request`
    /// can trigger shutdown via `OrchestratorRequest::Stop`.
    shutdown_tx: watch::Sender<bool>,
}

impl Orchestrator {
    /// `config_path` must be the actual file that was loaded — used for the
    /// FNV-1a hash in `DataPaths::new` so that `ao status` and the daemon
    /// agree on the storage root.
    pub async fn new(config: Config, config_path: &Path) -> Result<Self, OrchestratorError> {
        let config = Arc::new(config);
        let mut plugins = HashMap::new();

        for (project_id, project) in &config.projects {
            let agent_name = project.agent.as_deref().unwrap_or("claude-code");
            let runtime_name = project.runtime.as_deref().unwrap_or("tmux");

            let agent = create_agent(agent_name)
                .map_err(|e| OrchestratorError::RuntimeError(e.to_string()))?;
            let runtime = create_runtime(runtime_name, Arc::new(CommandRunner))
                .map_err(|e| OrchestratorError::RuntimeError(e.to_string()))?;

            let paths = DataPaths::new(config_path, project_id);
            paths.ensure_dirs().await?;

            let store = Arc::new(SessionStore::new(paths));
            let terminal_states = project.tracker.terminal_states.clone();
            let repo_root = PathBuf::from(shellexpand::tilde(&project.path).as_ref());

            plugins.insert(
                project_id.clone(),
                ProjectPlugins {
                    agent: Arc::from(agent),
                    runtime: Arc::from(runtime),
                    store,
                    terminal_states,
                    repo_root,
                },
            );
        }

        let graph = Arc::new(StateGraph::build());
        let socket_path = socket_path();
        let (shutdown_tx, _) = watch::channel(false);

        Ok(Self {
            config,
            plugins,
            graph,
            socket_path,
            shutdown_tx,
        })
    }

    /// Run the orchestrator: IPC listener + poll loop.
    /// Blocks until shutdown signal.
    pub async fn run(self) -> Result<(), OrchestratorError> {
        if self.socket_path.exists() {
            // Attempt connection -- if refused it's stale
            match tokio::net::UnixStream::connect(&self.socket_path).await {
                Ok(_) => return Err(OrchestratorError::AlreadyRunning(self.socket_path)),
                Err(_) => {
                    tokio::fs::remove_file(&self.socket_path).await.ok();
                }
            }
        }

        // Ensure parent directory exists
        if let Some(parent) = self.socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let listener = UnixListener::bind(&self.socket_path)?;
        info!(socket = ?self.socket_path, "orchestrator listening");

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let (ipc_tx, mut ipc_rx) = mpsc::channel::<IpcMsg>(64);

        // Spawn IPC listener task
        let ipc_tx2 = ipc_tx.clone();
        let mut shutdown_rx2 = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _)) => {
                                let tx = ipc_tx2.clone();
                                tokio::spawn(handle_connection(stream, tx));
                            }
                            Err(e) => {
                                error!(error = %e, "accept error");
                                break;
                            }
                        }
                    }
                    _ = shutdown_rx2.changed() => break,
                }
            }
        });

        // Ctrl-C / SIGTERM handler — also fires on `ao stop` via shutdown_tx
        let shutdown_tx_ctrlc = self.shutdown_tx.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("shutdown signal received");
            let _ = shutdown_tx_ctrlc.send(true);
        });

        // Main event loop: drain IPC channel between poll ticks
        let poll_interval = tokio::time::Duration::from_secs(30);
        let mut interval = tokio::time::interval(poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.poll_tick().await;
                }
                Some((req, reply_tx)) = ipc_rx.recv() => {
                    let response = self.handle_request(req).await;
                    let _ = reply_tx.send(response);
                }
                _ = shutdown_rx.changed() => {
                    info!("orchestrator shutting down");
                    break;
                }
            }
        }

        tokio::fs::remove_file(&self.socket_path).await.ok();
        Ok(())
    }

    async fn poll_tick(&self) {
        for (project_id, p) in &self.plugins {
            let tick = PollTick {
                graph: &self.graph,
                store: &p.store,
                agent: p.agent.clone(),
                runtime: p.runtime.clone(),
                tracker: Arc::new(NoOpTracker),
                terminal_states: p.terminal_states.clone(),
            };
            match tick.run().await {
                Ok(transitions) => {
                    for (id, from, to) in transitions {
                        info!(session_id = %id, %from, %to, project = %project_id, "poll transition");
                    }
                }
                Err(e) => {
                    error!(project = %project_id, error = %e, "poll tick error");
                }
            }
        }
    }

    async fn handle_request(&self, req: OrchestratorRequest) -> OrchestratorResponse {
        match req {
            OrchestratorRequest::Spawn {
                project_id,
                issue_url,
                agent: _,
                open: _,
            } => match self.handle_spawn(&project_id, &issue_url).await {
                Ok((session_id, branch, workspace_path)) => OrchestratorResponse::SpawnResult {
                    session_id,
                    branch,
                    workspace_path: workspace_path.to_string_lossy().to_string(),
                },
                Err(e) => error_response(e),
            },
            OrchestratorRequest::Stop => {
                let _ = self.shutdown_tx.send(true);
                OrchestratorResponse::Ok {
                    message: "stopping".to_string(),
                }
            }
            OrchestratorRequest::Kill { session_id } => match self.handle_kill(&session_id).await {
                Ok(()) => OrchestratorResponse::Ok {
                    message: format!("kill scheduled for {session_id}"),
                },
                Err(e) => error_response(e),
            },
            OrchestratorRequest::Cleanup {
                project_id,
                dry_run,
            } => match self.handle_cleanup(&project_id, dry_run).await {
                Ok((killed, skipped)) => OrchestratorResponse::CleanupResult { killed, skipped },
                Err(e) => error_response(e),
            },
            OrchestratorRequest::BatchSpawn {
                project_id,
                issue_urls,
                agent: _,
                open: _,
            } => {
                let results = self.handle_batch_spawn(&project_id, issue_urls).await;
                OrchestratorResponse::BatchSpawnResult { results }
            }
            OrchestratorRequest::Send {
                session_id,
                content,
                // MVP: no_wait and timeout_secs are reserved for post-MVP
                // busy-detection logic; the send is currently fire-and-forget.
                no_wait: _,
                timeout_secs: _,
            } => match self.handle_send(&session_id, &content).await {
                Ok(()) => OrchestratorResponse::SendResult {
                    delivered: true,
                    activity_state: "unknown".to_string(),
                },
                Err(e) => error_response(e),
            },
        }
    }

    async fn handle_spawn(
        &self,
        project_id: &str,
        issue_url: &str,
    ) -> Result<(String, String, PathBuf), OrchestratorError> {
        let p = self
            .plugins
            .get(project_id)
            .ok_or_else(|| OrchestratorError::ProjectNotFound(project_id.to_string()))?;

        let project = self.config.projects.get(project_id).unwrap();

        // Step 1: Pre-spawn tracker validation
        let runner = CommandRunner;
        let tracker = GitHubTracker::new(runner, project.repo.clone(), p.terminal_states.clone())
            .await
            .map_err(|e| OrchestratorError::TrackerError(e.to_string()))?;

        let issue = tracker
            .get_issue(issue_url)
            .await
            .map_err(|e| OrchestratorError::TrackerError(e.to_string()))?
            .ok_or_else(|| OrchestratorError::IssueNotFound(issue_url.to_string()))?;

        let state = issue["state"].as_str().unwrap_or("open");
        if classify_state(state, &p.terminal_states) == TrackerState::Terminal {
            return Err(OrchestratorError::IssueTerminal);
        }

        let issue_number = issue["number"].as_u64().unwrap_or(0);
        let title = issue["title"].as_str().unwrap_or("");
        let branch = tracker.branch_name(issue_number, title);

        // Step 2: Session ID
        let session_id = make_session_id(issue_url);

        // Step 3: Create session record
        p.store.paths_ref().ensure_dirs().await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let worktree_path = p.store.paths_ref().worktree_path(&session_id);

        let metadata = SessionMetadata {
            id: session_id.clone(),
            status: crate::types::SessionStatus::Spawning,
            termination_reason: None,
            issue_url: issue_url.to_string(),
            branch: branch.clone(),
            worktree_path: worktree_path.clone(),
            created_at: now,
            updated_at: now,
            attempts: 1,
            total_cost_usd: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            project_id: project_id.to_string(),
            agent: project
                .agent
                .as_deref()
                .unwrap_or("claude-code")
                .to_string(),
            runtime: project.runtime.as_deref().unwrap_or("tmux").to_string(),
            workspace: project
                .workspace
                .as_deref()
                .unwrap_or("worktree")
                .to_string(),
            tracker: project.tracker.plugin.clone(),
        };

        p.store.create_session(&metadata).await.map_err(|e| {
            if matches!(e, crate::session_store::StoreError::AlreadyExists(_)) {
                OrchestratorError::SessionExists(session_id.clone())
            } else {
                OrchestratorError::Store(e)
            }
        })?;

        // Step 4: Create workspace (git worktree)
        let wt_runner = CommandRunner;
        let out = wt_runner
            .run_in_dir(
                &[
                    "git",
                    "worktree",
                    "add",
                    &worktree_path.to_string_lossy(),
                    "-b",
                    &branch,
                ],
                &p.repo_root,
                None,
                None,
            )
            .await
            .map_err(|e| OrchestratorError::WorkspaceError(e.to_string()))?;

        if !out.success {
            let _ = p.store.delete_session(&session_id).await;
            return Err(OrchestratorError::WorkspaceError(out.stderr));
        }

        // Step 5: Get issue content + render prompt
        // On failure, remove the worktree created in step 4 to avoid orphans.
        let issue_content = match tracker.get_issue_content(issue_url).await {
            Ok(c) => c,
            Err(e) => {
                let _ = p.store.delete_session(&session_id).await;
                let _ = CommandRunner
                    .run_in_dir(
                        &[
                            "git",
                            "worktree",
                            "remove",
                            "--force",
                            &worktree_path.to_string_lossy(),
                        ],
                        &p.repo_root,
                        None,
                        None,
                    )
                    .await;
                return Err(OrchestratorError::TrackerError(e.to_string()));
            }
        };

        let templates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
        let prompt = if let Ok(engine) = crate::prompt::PromptEngine::new(&templates_dir) {
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
                issue_url: issue_url.to_string(),
                number: issue_content.number,
            };
            let project_ctx = ProjectContext {
                path: p.repo_root.clone(),
                name: project_id.to_string(),
            };
            engine
                .render_launch(&issue_ctx, &project_ctx, None)
                .await
                .unwrap_or_else(|_| "Complete the GitHub issue.".to_string())
        } else {
            "Complete the GitHub issue.".to_string()
        };

        // Step 6: Build and execute LaunchPlan
        let launch_ctx = crate::agent::LaunchContext {
            session_id: session_id.clone(),
            prompt,
            workspace_path: worktree_path.clone(),
            issue_id: issue_url.to_string(),
            branch: branch.clone(),
        };

        let plan = p.agent.launch_plan(&launch_ctx);
        for step in &plan.steps {
            if let Err(e) = p.runtime.execute_step(&session_id, step).await {
                let _ = p.store.delete_session(&session_id).await;
                let _ = CommandRunner
                    .run_in_dir(
                        &[
                            "git",
                            "worktree",
                            "remove",
                            "--force",
                            &worktree_path.to_string_lossy(),
                        ],
                        &p.repo_root,
                        None,
                        None,
                    )
                    .await;
                return Err(OrchestratorError::RuntimeError(e.to_string()));
            }
        }

        // Update status to Working
        let mut updated = metadata;
        updated.status = crate::types::SessionStatus::Working;
        updated.updated_at = now;
        p.store.write_metadata(&updated).await?;

        Ok((session_id, branch, worktree_path))
    }

    async fn handle_kill(&self, session_id: &str) -> Result<(), OrchestratorError> {
        for p in self.plugins.values() {
            if let Ok(mut meta) = p.store.read_metadata(session_id).await {
                meta.status = crate::types::SessionStatus::Killed;
                meta.updated_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                p.store.write_metadata(&meta).await?;
                let _ = p.runtime.destroy(session_id).await;
                return Ok(());
            }
        }
        Err(OrchestratorError::SessionNotFound(session_id.to_string()))
    }

    async fn handle_cleanup(
        &self,
        project_id: &str,
        dry_run: bool,
    ) -> Result<(Vec<String>, Vec<String>), OrchestratorError> {
        let p = self
            .plugins
            .get(project_id)
            .ok_or_else(|| OrchestratorError::ProjectNotFound(project_id.to_string()))?;
        let project = self.config.projects.get(project_id).unwrap();

        let runner = CommandRunner;
        let tracker = GitHubTracker::new(runner, project.repo.clone(), p.terminal_states.clone())
            .await
            .map_err(|e| OrchestratorError::TrackerError(e.to_string()))?;

        let active = p.store.list_active().await?;
        let mut killed = Vec::new();
        let mut skipped = Vec::new();

        for session in active {
            match tracker.get_issue(&session.issue_url).await {
                Ok(Some(issue)) => {
                    let state = issue["state"].as_str().unwrap_or("open");
                    if classify_state(state, &p.terminal_states) == TrackerState::Terminal {
                        if !dry_run {
                            let mut updated = session.clone();
                            updated.status = crate::types::SessionStatus::Killed;
                            let _ = p.store.write_metadata(&updated).await;
                            let _ = p.runtime.destroy(&session.id).await;
                        }
                        killed.push(session.id);
                    } else {
                        skipped.push(session.id);
                    }
                }
                _ => skipped.push(session.id),
            }
        }

        Ok((killed, skipped))
    }

    async fn handle_batch_spawn(
        &self,
        project_id: &str,
        issue_urls: Vec<String>,
    ) -> Vec<BatchSpawnItem> {
        let mut results = Vec::new();
        for issue_url in issue_urls {
            let outcome = match self.handle_spawn(project_id, &issue_url).await {
                Ok((session_id, branch, _)) => BatchSpawnOutcome::Spawned { session_id, branch },
                Err(OrchestratorError::SessionExists(id)) => BatchSpawnOutcome::Skipped {
                    reason: format!("session {id} already exists"),
                },
                Err(OrchestratorError::IssueTerminal) => BatchSpawnOutcome::Skipped {
                    reason: "issue is terminal".to_string(),
                },
                Err(e) => BatchSpawnOutcome::Failed {
                    error: e.to_string(),
                },
            };
            results.push(BatchSpawnItem { issue_url, outcome });
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
        results
    }

    async fn handle_send(&self, session_id: &str, content: &str) -> Result<(), OrchestratorError> {
        use crate::types::RuntimeStep;
        for p in self.plugins.values() {
            if p.store.read_metadata(session_id).await.is_ok() {
                p.runtime
                    .execute_step(
                        session_id,
                        &RuntimeStep::SendMessage {
                            content: content.to_string(),
                        },
                    )
                    .await
                    .map_err(|e| OrchestratorError::RuntimeError(e.to_string()))?;
                return Ok(());
            }
        }
        Err(OrchestratorError::SessionNotFound(session_id.to_string()))
    }
}

async fn handle_connection(mut stream: tokio::net::UnixStream, tx: mpsc::Sender<IpcMsg>) {
    let request: OrchestratorRequest = match read_message(&mut stream).await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "failed to read IPC request");
            return;
        }
    };
    let (reply_tx, reply_rx) = oneshot::channel();
    let _ = tx.send((request, reply_tx)).await;
    if let Ok(response) = reply_rx.await {
        let _ = write_message(&mut stream, &response).await;
    }
}

fn error_response(e: OrchestratorError) -> OrchestratorResponse {
    let code = match &e {
        OrchestratorError::IssueTerminal => "issue_terminal",
        OrchestratorError::IssueNotFound(_) => "issue_not_found",
        OrchestratorError::SessionExists(_) => "session_exists",
        OrchestratorError::SessionNotFound(_) => "session_not_found",
        OrchestratorError::ProjectNotFound(_) => "project_not_found",
        _ => "internal_error",
    };
    OrchestratorResponse::Error {
        code: code.to_string(),
        message: e.to_string(),
    }
}

/// Deterministic session ID from issue URL.
/// Normalizes the URL (lowercase, trim trailing slashes) before hashing so
/// that `…/issues/1` and `…/issues/1/` map to the same session.
/// Uses FNV-1a 64-bit hash -- stable across Rust versions and platforms.
pub fn make_session_id(issue_url: &str) -> String {
    let normalized = issue_url.to_lowercase();
    let normalized = normalized.trim_end_matches('/');
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in normalized.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("ao-{:x}", hash)
}

/// No-op tracker for poll tick (real tracker wiring is post-MVP).
struct NoOpTracker;

#[async_trait::async_trait]
impl crate::plugins::tracker::Tracker for NoOpTracker {
    async fn get_issue(
        &self,
        _: &str,
    ) -> Result<Option<serde_json::Value>, crate::plugins::tracker::TrackerError> {
        Ok(None)
    }
    fn branch_name(&self, n: u64, _: &str) -> String {
        format!("ao/issue-{n}")
    }
    fn issue_url(&self, n: u64) -> String {
        format!("#{n}")
    }
    async fn get_issue_content(
        &self,
        _: &str,
    ) -> Result<crate::plugins::tracker::IssueContent, crate::plugins::tracker::TrackerError> {
        Err(crate::plugins::tracker::TrackerError::NotFound(
            "noop".into(),
        ))
    }
    async fn add_comment(
        &self,
        _: &str,
        _: &str,
    ) -> Result<(), crate::plugins::tracker::TrackerError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ActivityState, Agent, GatherContext, LaunchContext};
    use crate::runtime::{Runtime, RuntimeError};
    use crate::types::{LaunchPlan, PluginMeta, RuntimeStep};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct MockAgent;
    impl Agent for MockAgent {
        fn meta(&self) -> PluginMeta {
            PluginMeta {
                name: "mock",
                version: "0.1",
                description: "test",
            }
        }
        fn launch_plan(&self, _ctx: &LaunchContext) -> LaunchPlan {
            LaunchPlan { steps: vec![] }
        }
        fn detect_activity(&self, _ctx: &GatherContext) -> ActivityState {
            ActivityState::Idle
        }
    }

    struct MockAgentWithStep;
    impl Agent for MockAgentWithStep {
        fn meta(&self) -> PluginMeta {
            PluginMeta {
                name: "mock-step",
                version: "0.1",
                description: "test",
            }
        }
        fn launch_plan(&self, _ctx: &LaunchContext) -> LaunchPlan {
            LaunchPlan {
                steps: vec![RuntimeStep::SendMessage {
                    content: "hello".to_string(),
                }],
            }
        }
        fn detect_activity(&self, _ctx: &GatherContext) -> ActivityState {
            ActivityState::Idle
        }
    }

    struct MockRuntime;
    #[async_trait]
    impl Runtime for MockRuntime {
        fn meta(&self) -> PluginMeta {
            PluginMeta {
                name: "mock",
                version: "0.1",
                description: "test",
            }
        }
        async fn execute_step(
            &self,
            _session_id: &str,
            _step: &RuntimeStep,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
        async fn get_output(
            &self,
            _session_id: &str,
            _lines: usize,
        ) -> Result<String, RuntimeError> {
            Ok(String::new())
        }
        async fn is_alive(&self, _session_id: &str) -> Result<bool, RuntimeError> {
            Ok(true)
        }
        async fn destroy(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn supported_steps(&self) -> &'static [&'static str] {
            &[]
        }
    }

    struct FailingRuntime;
    #[async_trait]
    impl Runtime for FailingRuntime {
        fn meta(&self) -> PluginMeta {
            PluginMeta {
                name: "failing",
                version: "0.1",
                description: "test",
            }
        }
        async fn execute_step(
            &self,
            _session_id: &str,
            _step: &RuntimeStep,
        ) -> Result<(), RuntimeError> {
            Err(RuntimeError::CommandFailed("test failure".to_string()))
        }
        async fn get_output(
            &self,
            _session_id: &str,
            _lines: usize,
        ) -> Result<String, RuntimeError> {
            Ok(String::new())
        }
        async fn is_alive(&self, _session_id: &str) -> Result<bool, RuntimeError> {
            Ok(false)
        }
        async fn destroy(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn supported_steps(&self) -> &'static [&'static str] {
            &[]
        }
    }

    async fn make_test_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let runner = CommandRunner;
        runner
            .run_in_dir(&["git", "init"], &path, None, None)
            .await
            .unwrap();
        runner
            .run_in_dir(
                &["git", "config", "user.email", "t@t.com"],
                &path,
                None,
                None,
            )
            .await
            .unwrap();
        runner
            .run_in_dir(&["git", "config", "user.name", "T"], &path, None, None)
            .await
            .unwrap();
        runner
            .run_in_dir(
                &["git", "commit", "--allow-empty", "-m", "init"],
                &path,
                None,
                None,
            )
            .await
            .unwrap();
        (dir, path)
    }

    /// Legacy-compatible orchestrator for walking skeleton tests.
    /// Uses DataPaths::from_root (flat root) with mock agent/runtime.
    struct LegacyOrchestrator {
        data_paths: DataPaths,
        repo_root: PathBuf,
        agent: Arc<dyn Agent>,
        runtime: Arc<dyn Runtime>,
    }

    impl LegacyOrchestrator {
        fn new(
            data_dir: &std::path::Path,
            repo_root: PathBuf,
            agent: Arc<dyn Agent>,
            runtime: Arc<dyn Runtime>,
        ) -> Self {
            let paths = DataPaths::from_root(data_dir.to_path_buf());
            Self {
                data_paths: paths,
                repo_root,
                agent,
                runtime,
            }
        }

        async fn spawn(
            &self,
            issue_url: &str,
            branch: &str,
            prompt: &str,
        ) -> Result<String, OrchestratorError> {
            let session_id = make_session_id(issue_url);

            self.data_paths.ensure_dirs().await?;
            let session_dir = self.data_paths.session_dir(&session_id);
            tokio::fs::create_dir(&session_dir).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    OrchestratorError::SessionExists(session_id.clone())
                } else {
                    OrchestratorError::Io(e)
                }
            })?;

            let mut worktree_created = false;
            match self
                .spawn_inner(
                    &session_id,
                    issue_url,
                    branch,
                    prompt,
                    &mut worktree_created,
                )
                .await
            {
                Ok(()) => Ok(session_id),
                Err(e) => {
                    self.unwind(&session_id, worktree_created).await;
                    Err(e)
                }
            }
        }

        async fn spawn_inner(
            &self,
            session_id: &str,
            issue_url: &str,
            branch: &str,
            prompt: &str,
            worktree_created: &mut bool,
        ) -> Result<(), OrchestratorError> {
            let worktree_path = self.data_paths.worktree_path(session_id);
            let runner = CommandRunner;
            let out = runner
                .run_in_dir(
                    &[
                        "git",
                        "worktree",
                        "add",
                        &worktree_path.to_string_lossy(),
                        "-b",
                        branch,
                    ],
                    &self.repo_root,
                    None,
                    None,
                )
                .await
                .map_err(|e| OrchestratorError::WorkspaceError(e.to_string()))?;

            if !out.success {
                return Err(OrchestratorError::WorkspaceError(out.stderr));
            }
            *worktree_created = true;

            let launch_ctx = crate::agent::LaunchContext {
                session_id: session_id.to_string(),
                prompt: prompt.to_string(),
                workspace_path: worktree_path,
                issue_id: issue_url.to_string(),
                branch: branch.to_string(),
            };

            let plan = self.agent.launch_plan(&launch_ctx);
            for step in &plan.steps {
                self.runtime
                    .execute_step(session_id, step)
                    .await
                    .map_err(|e| OrchestratorError::RuntimeError(e.to_string()))?;
            }

            Ok(())
        }

        async fn unwind(&self, session_id: &str, worktree_created: bool) {
            if worktree_created {
                let worktree_path = self.data_paths.worktree_path(session_id);
                let runner = CommandRunner;
                let _ = runner
                    .run_in_dir(
                        &[
                            "git",
                            "worktree",
                            "remove",
                            "--force",
                            &worktree_path.to_string_lossy(),
                        ],
                        &self.repo_root,
                        None,
                        None,
                    )
                    .await;
            }
            let session_dir = self.data_paths.session_dir(session_id);
            let _ = tokio::fs::remove_dir_all(&session_dir).await;
        }
    }

    fn make_legacy_orchestrator(
        data_dir: &std::path::Path,
        repo_root: PathBuf,
    ) -> LegacyOrchestrator {
        LegacyOrchestrator::new(
            data_dir,
            repo_root,
            Arc::new(MockAgent),
            Arc::new(MockRuntime),
        )
    }

    #[test]
    fn test_make_session_id_deterministic() {
        let id1 = make_session_id("https://github.com/org/repo/issues/1");
        let id2 = make_session_id("https://github.com/org/repo/issues/1");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("ao-"));
    }

    #[tokio::test]
    async fn test_spawn_creates_session_dir() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let data_dir = tempdir().unwrap();
        let orch = make_legacy_orchestrator(data_dir.path(), repo_root);

        let session_id = orch
            .spawn(
                "https://github.com/org/repo/issues/1",
                "feat/test-branch",
                "fix this",
            )
            .await
            .unwrap();

        let session_dir = orch.data_paths.session_dir(&session_id);
        assert!(session_dir.is_dir(), "session dir should exist");
    }

    #[tokio::test]
    async fn test_spawn_creates_worktree() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let data_dir = tempdir().unwrap();
        let orch = make_legacy_orchestrator(data_dir.path(), repo_root);

        let session_id = orch
            .spawn(
                "https://github.com/org/repo/issues/2",
                "feat/worktree-test",
                "fix that",
            )
            .await
            .unwrap();

        let worktree_path = orch.data_paths.worktree_path(&session_id);
        assert!(worktree_path.is_dir(), "worktree dir should exist");
    }

    #[tokio::test]
    async fn test_spawn_duplicate_fails() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let data_dir = tempdir().unwrap();
        let orch = make_legacy_orchestrator(data_dir.path(), repo_root);

        let issue_url = "https://github.com/org/repo/issues/3";
        orch.spawn(issue_url, "feat/dup-test", "fix").await.unwrap();

        let result = orch.spawn(issue_url, "feat/dup-test-2", "fix").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            OrchestratorError::SessionExists(_) => {}
            other => panic!("expected SessionExists, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_spawn_unwinds_on_plan_failure() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let data_dir = tempdir().unwrap();
        let orch = LegacyOrchestrator::new(
            data_dir.path(),
            repo_root,
            Arc::new(MockAgentWithStep),
            Arc::new(FailingRuntime),
        );

        let issue_url = "https://github.com/org/repo/issues/4";
        let result = orch.spawn(issue_url, "feat/fail-test", "fix").await;
        assert!(result.is_err());

        let session_id = make_session_id(issue_url);
        let session_dir = orch.data_paths.session_dir(&session_id);
        assert!(
            !session_dir.exists(),
            "session dir should not exist after unwind"
        );
    }

    #[test]
    fn test_error_response_codes() {
        let resp = error_response(OrchestratorError::IssueTerminal);
        match resp {
            OrchestratorResponse::Error { code, .. } => assert_eq!(code, "issue_terminal"),
            _ => panic!("expected Error response"),
        }

        let resp = error_response(OrchestratorError::SessionNotFound("x".into()));
        match resp {
            OrchestratorResponse::Error { code, .. } => assert_eq!(code, "session_not_found"),
            _ => panic!("expected Error response"),
        }
    }
}
