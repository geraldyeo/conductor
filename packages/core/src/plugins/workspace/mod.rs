use async_trait::async_trait;
use std::path::PathBuf;
use thiserror::Error;

pub mod worktree;

pub use worktree::WorktreeWorkspace;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace already exists: {0}")]
    AlreadyExists(String),
    #[error("workspace not found: {0}")]
    NotFound(String),
    #[error("symlink escape detected: {0}")]
    SymlinkEscape(String),
    #[error("origin collision: {0}")]
    OriginCollision(String),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub id: String,
    pub path: PathBuf,
    pub branch: String,
    pub head_sha: String,
}

#[async_trait]
pub trait Workspace: Send + Sync {
    async fn create(&self, id: &str, branch: &str) -> Result<WorkspaceInfo, WorkspaceError>;
    async fn destroy(&self, id: &str, force: bool) -> Result<(), WorkspaceError>;
    async fn exists(&self, id: &str) -> bool;
    async fn info(&self, id: &str) -> Result<WorkspaceInfo, WorkspaceError>;
    async fn list(&self) -> Result<Vec<WorkspaceInfo>, WorkspaceError>;
}
