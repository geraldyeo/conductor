mod spawn;

pub use spawn::{spawn_session, SpawnError, SpawnRequest};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::tracker::{IssueContent, Tracker, TrackerError};
    use crate::plugins::workspace::{Workspace, WorkspaceError, WorkspaceInfo};
    use crate::store::SessionStore;
    use crate::types::SessionStatus;
    use crate::utils::DataPaths;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use tempfile::tempdir;

    // --- Mock Tracker ---
    struct MockTracker {
        issue_content: Option<IssueContent>,
    }

    impl MockTracker {
        fn new_open() -> Self {
            Self {
                issue_content: Some(IssueContent {
                    title: "Fix the bug".to_string(),
                    body: "There is a bug".to_string(),
                    comments: vec![],
                    state: "OPEN".to_string(),
                    labels: vec!["bug".to_string()],
                    assignees: vec![],
                    author: "alice".to_string(),
                    number: 42,
                }),
            }
        }

        fn new_closed() -> Self {
            Self {
                issue_content: Some(IssueContent {
                    title: "Fix the bug".to_string(),
                    body: "There is a bug".to_string(),
                    comments: vec![],
                    state: "CLOSED".to_string(),
                    labels: vec![],
                    assignees: vec![],
                    author: "alice".to_string(),
                    number: 42,
                }),
            }
        }

        fn new_not_found() -> Self {
            Self {
                issue_content: None,
            }
        }
    }

    #[async_trait]
    impl Tracker for MockTracker {
        async fn get_issue(
            &self,
            _issue_url: &str,
        ) -> Result<Option<serde_json::Value>, TrackerError> {
            match &self.issue_content {
                Some(content) => Ok(Some(serde_json::json!({
                    "number": content.number,
                    "title": content.title,
                    "body": content.body,
                    "state": content.state,
                    "labels": content.labels.iter().map(|l| serde_json::json!({"name": l})).collect::<Vec<_>>(),
                    "assignees": [],
                    "author": {"login": content.author},
                    "comments": [],
                }))),
                None => Ok(None),
            }
        }

        fn branch_name(&self, issue_number: u64, title: &str) -> String {
            let slug: String = title
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-");
            format!("{issue_number}-{slug}")
        }

        fn issue_url(&self, issue_number: u64) -> String {
            format!("https://github.com/test/repo/issues/{issue_number}")
        }

        async fn get_issue_content(
            &self,
            _issue_url: &str,
        ) -> Result<IssueContent, TrackerError> {
            self.issue_content
                .clone()
                .ok_or_else(|| TrackerError::NotFound("not found".to_string()))
        }

        async fn add_comment(
            &self,
            _issue_url: &str,
            _body: &str,
        ) -> Result<(), TrackerError> {
            Ok(())
        }
    }

    // --- Mock Workspace ---
    struct MockWorkspace {
        worktrees_dir: PathBuf,
    }

    impl MockWorkspace {
        fn new(dir: &std::path::Path) -> Self {
            Self {
                worktrees_dir: dir.to_path_buf(),
            }
        }
    }

    #[async_trait]
    impl Workspace for MockWorkspace {
        async fn create(
            &self,
            id: &str,
            branch: &str,
        ) -> Result<WorkspaceInfo, WorkspaceError> {
            let path = self.worktrees_dir.join(id);
            tokio::fs::create_dir_all(&path).await?;
            Ok(WorkspaceInfo {
                id: id.to_string(),
                path,
                branch: branch.to_string(),
                head_sha: "abc123".to_string(),
            })
        }

        async fn destroy(&self, id: &str, _force: bool) -> Result<(), WorkspaceError> {
            let path = self.worktrees_dir.join(id);
            if path.exists() {
                tokio::fs::remove_dir_all(&path).await?;
            }
            Ok(())
        }

        async fn exists(&self, id: &str) -> bool {
            self.worktrees_dir.join(id).is_dir()
        }

        async fn info(&self, id: &str) -> Result<WorkspaceInfo, WorkspaceError> {
            let path = self.worktrees_dir.join(id);
            if !path.is_dir() {
                return Err(WorkspaceError::NotFound(id.to_string()));
            }
            Ok(WorkspaceInfo {
                id: id.to_string(),
                path,
                branch: "test-branch".to_string(),
                head_sha: "abc123".to_string(),
            })
        }

        async fn list(&self) -> Result<Vec<WorkspaceInfo>, WorkspaceError> {
            Ok(vec![])
        }
    }

    fn make_spawn_request(data_paths: DataPaths) -> SpawnRequest {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let templates_dir = manifest_dir.join("templates");
        SpawnRequest {
            issue_url: "https://github.com/test/repo/issues/42".to_string(),
            project_name: "test-project".to_string(),
            project_path: PathBuf::from("/tmp/project"),
            session_prefix: "test".to_string(),
            terminal_states: vec!["closed".to_string()],
            data_paths,
            templates_dir: Some(templates_dir),
        }
    }

    #[tokio::test]
    async fn test_spawn_rejects_terminal_issue() {
        let dir = tempdir().unwrap();
        let paths = DataPaths::from_root(dir.path().to_path_buf());
        let store = SessionStore::new(paths.clone());
        let tracker = MockTracker::new_closed();
        let workspace = MockWorkspace::new(&dir.path().join("worktrees"));
        let req = make_spawn_request(paths);

        let result = spawn_session(&req, &tracker, &store, &workspace).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SpawnError::IssueTerminal(_) => {}
            other => panic!("expected IssueTerminal, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_spawn_rejects_missing_issue() {
        let dir = tempdir().unwrap();
        let paths = DataPaths::from_root(dir.path().to_path_buf());
        let store = SessionStore::new(paths.clone());
        let tracker = MockTracker::new_not_found();
        let workspace = MockWorkspace::new(&dir.path().join("worktrees"));
        let req = make_spawn_request(paths);

        let result = spawn_session(&req, &tracker, &store, &workspace).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SpawnError::IssueNotFound(_) => {}
            other => panic!("expected IssueNotFound, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_spawn_creates_session_and_workspace() {
        let dir = tempdir().unwrap();
        let paths = DataPaths::from_root(dir.path().to_path_buf());
        let store = SessionStore::new(paths.clone());
        let tracker = MockTracker::new_open();
        let workspace = MockWorkspace::new(&dir.path().join("worktrees"));
        let req = make_spawn_request(paths);

        let result = spawn_session(&req, &tracker, &store, &workspace).await;
        assert!(result.is_ok(), "spawn failed: {:?}", result.err());
        let session_id = result.unwrap();

        // Verify session was created
        let meta = store.read(&session_id).await.unwrap();
        assert_eq!(meta.status, SessionStatus::Working);
        assert_eq!(meta.issue_id, "42");
        assert_eq!(meta.agent, "claude-code");
        assert_eq!(meta.runtime, "tmux");

        // Verify workspace was created
        assert!(workspace.exists(&session_id).await);
    }

    #[tokio::test]
    async fn test_spawn_session_id_format() {
        let dir = tempdir().unwrap();
        let paths = DataPaths::from_root(dir.path().to_path_buf());
        let store = SessionStore::new(paths.clone());
        let tracker = MockTracker::new_open();
        let workspace = MockWorkspace::new(&dir.path().join("worktrees"));
        let req = make_spawn_request(paths);

        let session_id = spawn_session(&req, &tracker, &store, &workspace)
            .await
            .unwrap();
        // Format: {prefix}-{issueNumber}-{attempt}
        assert!(
            session_id.starts_with("test-42-"),
            "session_id should start with 'test-42-', got: {session_id}"
        );
    }

    #[tokio::test]
    async fn test_spawn_writes_journal_entry() {
        let dir = tempdir().unwrap();
        let paths = DataPaths::from_root(dir.path().to_path_buf());
        let store = SessionStore::new(paths.clone());
        let tracker = MockTracker::new_open();
        let workspace = MockWorkspace::new(&dir.path().join("worktrees"));
        let req = make_spawn_request(paths);

        let session_id = spawn_session(&req, &tracker, &store, &workspace)
            .await
            .unwrap();
        let journal = store.read_journal(&session_id).await.unwrap();
        assert!(!journal.is_empty(), "journal should have entries");
    }
}
