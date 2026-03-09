use crate::agent::Agent;
use crate::runtime::Runtime;
use crate::utils::{CommandRunner, DataPaths};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("issue is in terminal state")]
    IssueTerminal,
    #[error("issue not found: {0}")]
    IssueNotFound(String),
    #[error("session already exists: {0}")]
    SessionExists(String),
    #[error("workspace error: {0}")]
    WorkspaceError(String),
    #[error("runtime error: {0}")]
    RuntimeError(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configuration for the Orchestrator.
pub struct OrchestratorConfig {
    pub data_paths: DataPaths,
    pub repo_root: PathBuf,
}

pub struct Orchestrator {
    config: OrchestratorConfig,
    agent: Arc<dyn Agent>,
    runtime: Arc<dyn Runtime>,
}

/// Deterministic session ID from issue URL.
fn make_session_id(issue_url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    issue_url.hash(&mut h);
    format!("ao-{:x}", h.finish())
}

impl Orchestrator {
    pub fn new(
        config: OrchestratorConfig,
        agent: Arc<dyn Agent>,
        runtime: Arc<dyn Runtime>,
    ) -> Self {
        Self {
            config,
            agent,
            runtime,
        }
    }

    /// Execute the 8-step spawn sequence. Returns the session_id on success.
    pub async fn spawn(
        &self,
        issue_url: &str,
        branch: &str,
        prompt: &str,
    ) -> Result<String, OrchestratorError> {
        // Step 1: Validate issue — for MVP, skip real tracker validation.
        let session_id = make_session_id(issue_url);
        tracing::info!(session_id = %session_id, issue_url = %issue_url, "spawn: step 1 — validate issue (skipped at MVP)");

        // Step 2: Create session record
        self.config
            .data_paths
            .ensure_dirs()
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let session_dir = self.config.data_paths.session_dir(&session_id);
        tokio::fs::create_dir(&session_dir).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                OrchestratorError::SessionExists(session_id.clone())
            } else {
                OrchestratorError::Io(e)
            }
        })?;
        tracing::info!(session_id = %session_id, "spawn: step 2 — session record created");

        // Steps 3-8 with unwind on failure
        let mut worktree_created = false;
        match self
            .spawn_inner(&session_id, issue_url, branch, prompt, &mut worktree_created)
            .await
        {
            Ok(()) => Ok(session_id),
            Err(e) => {
                tracing::error!(session_id = %session_id, error = %e, "spawn failed, unwinding");
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
        // Step 3: Create workspace (git worktree add)
        let worktree_path = self.config.data_paths.worktree_path(session_id);
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
                &self.config.repo_root,
                None,
                None,
            )
            .await
            .map_err(|e| OrchestratorError::WorkspaceError(e.to_string()))?;

        if !out.success {
            return Err(OrchestratorError::WorkspaceError(out.stderr));
        }
        *worktree_created = true;
        tracing::info!(session_id = %session_id, "spawn: step 3 — worktree created");

        // Steps 4-5: Hooks (stubbed at MVP)
        tracing::info!(session_id = %session_id, "spawn: steps 4-5 — hooks (skipped at MVP)");

        // Step 6: Build LaunchContext
        let launch_ctx = crate::agent::LaunchContext {
            session_id: session_id.to_string(),
            prompt: prompt.to_string(),
            workspace_path: worktree_path,
            issue_id: issue_url.to_string(),
            branch: branch.to_string(),
        };
        tracing::info!(session_id = %session_id, "spawn: step 6 — launch context built");

        // Step 7: Build LaunchPlan
        let plan = self.agent.launch_plan(&launch_ctx);
        tracing::info!(session_id = %session_id, steps = plan.steps.len(), "spawn: step 7 — launch plan built");

        // Step 8: Execute LaunchPlan
        for step in &plan.steps {
            self.runtime
                .execute_step(session_id, step)
                .await
                .map_err(|e| OrchestratorError::RuntimeError(e.to_string()))?;
        }
        tracing::info!(session_id = %session_id, "spawn: step 8 — launch plan executed, status → Working");

        Ok(())
    }

    /// Best-effort cleanup on spawn failure.
    async fn unwind(&self, session_id: &str, worktree_created: bool) {
        if worktree_created {
            let worktree_path = self.config.data_paths.worktree_path(session_id);
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
                    &self.config.repo_root,
                    None,
                    None,
                )
                .await;
        }
        let session_dir = self.config.data_paths.session_dir(session_id);
        let _ = tokio::fs::remove_dir_all(&session_dir).await;
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
        let runner = CommandRunner::default();
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
            .run_in_dir(
                &["git", "config", "user.name", "T"],
                &path,
                None,
                None,
            )
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

    fn make_orchestrator(
        data_dir: &std::path::Path,
        repo_root: PathBuf,
    ) -> Orchestrator {
        let paths = DataPaths::from_root(data_dir.to_path_buf());
        Orchestrator::new(
            OrchestratorConfig {
                data_paths: paths,
                repo_root,
            },
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
        let orch = make_orchestrator(data_dir.path(), repo_root);

        let session_id = orch
            .spawn(
                "https://github.com/org/repo/issues/1",
                "feat/test-branch",
                "fix this",
            )
            .await
            .unwrap();

        let session_dir = orch.config.data_paths.session_dir(&session_id);
        assert!(session_dir.is_dir(), "session dir should exist");
    }

    #[tokio::test]
    async fn test_spawn_creates_worktree() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let data_dir = tempdir().unwrap();
        let orch = make_orchestrator(data_dir.path(), repo_root);

        let session_id = orch
            .spawn(
                "https://github.com/org/repo/issues/2",
                "feat/worktree-test",
                "fix that",
            )
            .await
            .unwrap();

        let worktree_path = orch.config.data_paths.worktree_path(&session_id);
        assert!(worktree_path.is_dir(), "worktree dir should exist");
    }

    #[tokio::test]
    async fn test_spawn_duplicate_fails() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let data_dir = tempdir().unwrap();
        let orch = make_orchestrator(data_dir.path(), repo_root);

        let issue_url = "https://github.com/org/repo/issues/3";
        orch.spawn(issue_url, "feat/dup-test", "fix")
            .await
            .unwrap();

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
        let paths = DataPaths::from_root(data_dir.path().to_path_buf());
        let orch = Orchestrator::new(
            OrchestratorConfig {
                data_paths: paths,
                repo_root,
            },
            Arc::new(MockAgentWithStep),
            Arc::new(FailingRuntime),
        );

        let issue_url = "https://github.com/org/repo/issues/4";
        let result = orch.spawn(issue_url, "feat/fail-test", "fix").await;
        assert!(result.is_err());

        // Session dir should be cleaned up (unwound)
        let session_id = make_session_id(issue_url);
        let session_dir = orch.config.data_paths.session_dir(&session_id);
        assert!(
            !session_dir.exists(),
            "session dir should not exist after unwind"
        );
    }
}
