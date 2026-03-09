use super::{Workspace, WorkspaceError, WorkspaceInfo};
use crate::utils::{CommandRunner, DataPaths};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tracing::warn;

pub struct WorktreeWorkspace {
    runner: CommandRunner,
    paths: DataPaths,
    repo_root: PathBuf,
}

impl WorktreeWorkspace {
    pub fn new(runner: CommandRunner, paths: DataPaths, repo_root: PathBuf) -> Self {
        Self {
            runner,
            paths,
            repo_root,
        }
    }
}

fn prevent_symlink_escape(base: &Path, target: &Path) -> Result<(), WorkspaceError> {
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let canonical_target = if target.exists() {
        target.canonicalize().unwrap_or_else(|_| target.to_path_buf())
    } else {
        let parent = target.parent().unwrap_or(target);
        let canonical_parent = parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf());
        canonical_parent.join(target.file_name().unwrap_or_default())
    };
    if !canonical_target.starts_with(&canonical_base) {
        return Err(WorkspaceError::SymlinkEscape(format!(
            "{} escapes {}",
            target.display(),
            base.display()
        )));
    }
    Ok(())
}

#[async_trait]
impl Workspace for WorktreeWorkspace {
    async fn create(&self, id: &str, branch: &str) -> Result<WorkspaceInfo, WorkspaceError> {
        let worktrees_dir = self.paths.worktrees_dir();
        let worktree_path = self.paths.worktree_path(id);

        // Ensure worktrees dir exists so canonicalize works
        tokio::fs::create_dir_all(&worktrees_dir).await?;

        // Symlink escape check
        prevent_symlink_escape(&worktrees_dir, &worktree_path)?;

        // .origin collision detection
        let origin_file = worktrees_dir.join(".origin");
        let repo_root_str = self.repo_root.to_string_lossy().into_owned();
        if origin_file.exists() {
            let existing = tokio::fs::read_to_string(&origin_file).await?;
            let existing = existing.trim();
            if existing != repo_root_str {
                return Err(WorkspaceError::OriginCollision(format!(
                    "worktrees dir is owned by repo '{}', cannot use for '{}'",
                    existing, repo_root_str
                )));
            }
        } else {
            tokio::fs::write(&origin_file, &repo_root_str).await?;
        }

        // git worktree add <worktree_path> -b <branch>
        let worktree_path_str = worktree_path.to_string_lossy().into_owned();
        let out = self
            .runner
            .run_in_dir(
                &[
                    "git",
                    "worktree",
                    "add",
                    &worktree_path_str,
                    "-b",
                    branch,
                ],
                &self.repo_root,
                None,
                None,
            )
            .await
            .map_err(|e| WorkspaceError::CommandFailed(e.to_string()))?;

        if !out.success {
            if out.stderr.contains("already exists") {
                return Err(WorkspaceError::AlreadyExists(id.to_string()));
            }
            return Err(WorkspaceError::CommandFailed(format!(
                "git worktree add failed: {}",
                out.stderr.trim()
            )));
        }

        // Get HEAD SHA
        let sha_out = self
            .runner
            .run_in_dir(&["git", "rev-parse", "HEAD"], &worktree_path, None, None)
            .await
            .map_err(|e| WorkspaceError::CommandFailed(e.to_string()))?;

        if !sha_out.success {
            return Err(WorkspaceError::CommandFailed(format!(
                "git rev-parse HEAD failed: {}",
                sha_out.stderr.trim()
            )));
        }

        Ok(WorkspaceInfo {
            id: id.to_string(),
            path: worktree_path,
            branch: branch.to_string(),
            head_sha: sha_out.stdout.trim().to_string(),
        })
    }

    async fn destroy(&self, id: &str, force: bool) -> Result<(), WorkspaceError> {
        let worktree_path = self.paths.worktree_path(id);
        let worktree_path_str = worktree_path.to_string_lossy().into_owned();

        // Get branch name before removing worktree
        let branch = if worktree_path.exists() {
            let branch_out = self
                .runner
                .run_in_dir(
                    &["git", "branch", "--show-current"],
                    &worktree_path,
                    None,
                    None,
                )
                .await
                .ok()
                .filter(|o| o.success)
                .map(|o| o.stdout.trim().to_string());
            branch_out
        } else {
            None
        };

        // git worktree remove [--force] <path>
        let mut args = vec!["git", "worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&worktree_path_str);

        let out = self
            .runner
            .run_in_dir(&args, &self.repo_root, None, None)
            .await
            .map_err(|e| WorkspaceError::CommandFailed(e.to_string()))?;

        if !out.success {
            return Err(WorkspaceError::CommandFailed(format!(
                "git worktree remove failed: {}",
                out.stderr.trim()
            )));
        }

        // Delete the branch
        if let Some(branch_name) = branch {
            if !branch_name.is_empty() {
                let flag = if force { "-D" } else { "-d" };
                let branch_out = self
                    .runner
                    .run_in_dir(
                        &["git", "branch", flag, &branch_name],
                        &self.repo_root,
                        None,
                        None,
                    )
                    .await;
                match branch_out {
                    Ok(o) if !o.success => {
                        warn!(
                            branch = %branch_name,
                            stderr = %o.stderr.trim(),
                            "branch deletion failed, swallowing error"
                        );
                    }
                    Err(e) => {
                        warn!(
                            branch = %branch_name,
                            error = %e,
                            "branch deletion command error, swallowing"
                        );
                    }
                    Ok(_) => {}
                }
            }
        }

        Ok(())
    }

    async fn exists(&self, id: &str) -> bool {
        self.paths.worktree_path(id).is_dir()
    }

    async fn info(&self, id: &str) -> Result<WorkspaceInfo, WorkspaceError> {
        let worktree_path = self.paths.worktree_path(id);
        if !worktree_path.is_dir() {
            return Err(WorkspaceError::NotFound(id.to_string()));
        }

        let branch_out = self
            .runner
            .run_in_dir(
                &["git", "branch", "--show-current"],
                &worktree_path,
                None,
                None,
            )
            .await
            .map_err(|e| WorkspaceError::CommandFailed(e.to_string()))?;

        if !branch_out.success {
            return Err(WorkspaceError::CommandFailed(format!(
                "git branch --show-current failed: {}",
                branch_out.stderr.trim()
            )));
        }

        let sha_out = self
            .runner
            .run_in_dir(&["git", "rev-parse", "HEAD"], &worktree_path, None, None)
            .await
            .map_err(|e| WorkspaceError::CommandFailed(e.to_string()))?;

        if !sha_out.success {
            return Err(WorkspaceError::CommandFailed(format!(
                "git rev-parse HEAD failed: {}",
                sha_out.stderr.trim()
            )));
        }

        Ok(WorkspaceInfo {
            id: id.to_string(),
            path: worktree_path,
            branch: branch_out.stdout.trim().to_string(),
            head_sha: sha_out.stdout.trim().to_string(),
        })
    }

    async fn list(&self) -> Result<Vec<WorkspaceInfo>, WorkspaceError> {
        let worktrees_dir = self.paths.worktrees_dir();

        let out = self
            .runner
            .run_in_dir(
                &["git", "worktree", "list", "--porcelain"],
                &self.repo_root,
                None,
                None,
            )
            .await
            .map_err(|e| WorkspaceError::CommandFailed(e.to_string()))?;

        if !out.success {
            return Err(WorkspaceError::CommandFailed(format!(
                "git worktree list failed: {}",
                out.stderr.trim()
            )));
        }

        let mut result = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_head: Option<String> = None;
        let mut current_branch: Option<String> = None;

        for line in out.stdout.lines() {
            if let Some(rest) = line.strip_prefix("worktree ") {
                // Save previous entry if any
                if let (Some(path), Some(head)) = (current_path.take(), current_head.take()) {
                    if path.starts_with(&worktrees_dir) {
                        let id = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let branch = current_branch.take().unwrap_or_default();
                        result.push(WorkspaceInfo {
                            id,
                            path,
                            branch,
                            head_sha: head,
                        });
                    }
                }
                current_path = Some(PathBuf::from(rest));
                current_head = None;
                current_branch = None;
            } else if let Some(rest) = line.strip_prefix("HEAD ") {
                current_head = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
                current_branch = Some(rest.to_string());
            } else if line.is_empty() {
                // blank line separates entries — flush current
                if let (Some(path), Some(head)) = (current_path.take(), current_head.take()) {
                    if path.starts_with(&worktrees_dir) {
                        let id = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let branch = current_branch.take().unwrap_or_default();
                        result.push(WorkspaceInfo {
                            id,
                            path,
                            branch,
                            head_sha: head,
                        });
                    } else {
                        current_branch = None;
                    }
                }
            }
        }

        // Handle last entry (no trailing blank line)
        if let (Some(path), Some(head)) = (current_path.take(), current_head.take()) {
            if path.starts_with(&worktrees_dir) {
                let id = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let branch = current_branch.unwrap_or_default();
                result.push(WorkspaceInfo {
                    id,
                    path,
                    branch,
                    head_sha: head,
                });
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::{CommandRunner, DataPaths};
    use tempfile::tempdir;

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
                &["git", "config", "user.email", "test@test.com"],
                &path,
                None,
                None,
            )
            .await
            .unwrap();
        runner
            .run_in_dir(
                &["git", "config", "user.name", "Test"],
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

    fn make_workspace(repo_root: PathBuf) -> (tempfile::TempDir, WorktreeWorkspace) {
        let data_dir = tempdir().unwrap();
        let paths = DataPaths::from_root(data_dir.path().to_path_buf());
        let ws = WorktreeWorkspace::new(CommandRunner::default(), paths, repo_root);
        (data_dir, ws)
    }

    #[tokio::test]
    async fn test_create_and_exists() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let (_data_dir, ws) = make_workspace(repo_root);

        assert!(!ws.exists("test-session-1").await);
        ws.create("test-session-1", "feat/test-branch-1")
            .await
            .unwrap();
        assert!(ws.exists("test-session-1").await);
    }

    #[tokio::test]
    async fn test_info_returns_correct_branch() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let (_data_dir, ws) = make_workspace(repo_root);

        ws.create("test-session-2", "feat/info-branch")
            .await
            .unwrap();
        let info = ws.info("test-session-2").await.unwrap();
        assert_eq!(info.branch, "feat/info-branch");
        assert_eq!(info.id, "test-session-2");
        assert!(!info.head_sha.is_empty());
    }

    #[tokio::test]
    async fn test_exists_false_for_missing() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let (_data_dir, ws) = make_workspace(repo_root);

        assert!(!ws.exists("nonexistent-session").await);
    }

    #[tokio::test]
    async fn test_symlink_escape_prevention() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let data_dir = tempdir().unwrap();
        let paths = DataPaths::from_root(data_dir.path().to_path_buf());

        // Ensure worktrees dir exists for canonicalization
        tokio::fs::create_dir_all(paths.worktrees_dir()).await.unwrap();

        let worktrees_dir = paths.worktrees_dir();
        // Construct a path that escapes the worktrees dir via ".."
        let escape_path = worktrees_dir.join("..").join("etc").join("passwd");

        let result = prevent_symlink_escape(&worktrees_dir, &escape_path);
        assert!(
            matches!(result, Err(WorkspaceError::SymlinkEscape(_))),
            "Expected SymlinkEscape error, got: {:?}",
            result
        );

        // A valid path should pass
        let valid_path = worktrees_dir.join("valid-session");
        let result = prevent_symlink_escape(&worktrees_dir, &valid_path);
        assert!(result.is_ok(), "Expected Ok for valid path, got: {:?}", result);

        drop(repo_root); // keep repo alive
    }

    #[tokio::test]
    async fn test_destroy_removes_worktree() {
        let (_repo_dir, repo_root) = make_test_repo().await;
        let (_data_dir, ws) = make_workspace(repo_root);

        ws.create("test-session-3", "feat/destroy-branch")
            .await
            .unwrap();
        assert!(ws.exists("test-session-3").await);

        ws.destroy("test-session-3", true).await.unwrap();
        assert!(!ws.exists("test-session-3").await);
    }
}
