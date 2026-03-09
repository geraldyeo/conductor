use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DataPathsError {
    #[error("io error creating directories: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    /// Construct from a pre-computed root path.
    pub fn from_root(root: PathBuf) -> Self {
        Self { root }
    }

    /// Compute root from config path and project ID.
    /// Format: ~/.agent-orchestrator/{hash-12chars}-{project_id}/
    /// Uses FNV-1a 64-bit hash — stable across Rust versions and platforms.
    pub fn new(config_path: &Path, project_id: &str) -> Self {
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in config_path.to_string_lossy().bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let hash_str = format!("{:016x}", hash);
        let dir_name = format!("{}-{}", &hash_str[..12], project_id);
        let root = dirs_next_home().join(".agent-orchestrator").join(dir_name);
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn origin_file(&self) -> PathBuf {
        self.root.join(".origin")
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    pub fn session_dir(&self, id: &str) -> PathBuf {
        self.sessions_dir().join(id)
    }

    pub fn metadata_file(&self, id: &str) -> PathBuf {
        self.session_dir(id).join("metadata")
    }

    pub fn journal_file(&self, id: &str) -> PathBuf {
        self.session_dir(id).join("journal.jsonl")
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.root.join("worktrees")
    }

    pub fn worktree_path(&self, id: &str) -> PathBuf {
        self.worktrees_dir().join(id)
    }

    pub fn archive_dir(&self) -> PathBuf {
        self.root.join("archive")
    }

    pub async fn ensure_dirs(&self) -> Result<(), DataPathsError> {
        tokio::fs::create_dir_all(self.sessions_dir()).await?;
        tokio::fs::create_dir_all(self.worktrees_dir()).await?;
        tokio::fs::create_dir_all(self.archive_dir()).await?;
        Ok(())
    }
}

fn dirs_next_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_paths_are_under_root() {
        let root = PathBuf::from("/tmp/ao-test/abc123-myproj");
        let paths = DataPaths::from_root(root.clone());
        assert!(paths.sessions_dir().starts_with(&root));
        assert!(paths.worktrees_dir().starts_with(&root));
        assert!(paths.archive_dir().starts_with(&root));
    }

    #[test]
    fn test_session_paths() {
        let paths = DataPaths::from_root(PathBuf::from("/tmp/ao"));
        assert_eq!(
            paths.session_dir("myproj-42-1"),
            PathBuf::from("/tmp/ao/sessions/myproj-42-1")
        );
        assert_eq!(
            paths.metadata_file("myproj-42-1"),
            PathBuf::from("/tmp/ao/sessions/myproj-42-1/metadata")
        );
    }

    #[tokio::test]
    async fn test_ensure_dirs_creates_directories() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("ao-data");
        let paths = DataPaths::from_root(root.clone());
        paths.ensure_dirs().await.unwrap();
        assert!(paths.sessions_dir().exists());
        assert!(paths.worktrees_dir().exists());
        assert!(paths.archive_dir().exists());
    }
}
