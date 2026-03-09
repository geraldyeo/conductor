use crate::types::{SessionStatus, TerminationReason};
use crate::utils::DataPaths;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("session already exists: {0}")]
    AlreadyExists(String),
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Session metadata persisted to disk as KEY=VALUE (not serde).
/// Values have newlines escaped as \n and backslashes as \\.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionMetadata {
    pub id: String,
    pub status: SessionStatus,
    pub termination_reason: Option<TerminationReason>,
    pub issue_url: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub created_at: u64,
    pub updated_at: u64,
    pub attempts: u32,
    pub total_cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub project_id: String,
    pub agent: String,
    pub runtime: String,
    pub workspace: String,
    pub tracker: String,
}

/// Journal entry for the JSONL audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub timestamp: u64,
    pub event: String,
    pub data: HashMap<String, serde_json::Value>,
}

/// Escape a value for KEY=VALUE format.
/// `\` → `\\`, newline → `\n`
fn escape_value(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

/// Unescape a value from KEY=VALUE format.
/// `\\` → `\`, `\n` → newline
fn unescape_value(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('n') => result.push('\n'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn serialize_metadata(m: &SessionMetadata) -> String {
    let mut lines = Vec::with_capacity(17);
    lines.push(format!("id={}", escape_value(&m.id)));
    lines.push(format!("status={}", m.status));
    lines.push(format!(
        "termination_reason={}",
        m.termination_reason
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_default()
    ));
    lines.push(format!("issue_url={}", escape_value(&m.issue_url)));
    lines.push(format!("branch={}", escape_value(&m.branch)));
    lines.push(format!(
        "worktree_path={}",
        escape_value(&m.worktree_path.to_string_lossy())
    ));
    lines.push(format!("created_at={}", m.created_at));
    lines.push(format!("updated_at={}", m.updated_at));
    lines.push(format!("attempts={}", m.attempts));
    lines.push(format!("total_cost_usd={}", m.total_cost_usd));
    lines.push(format!("input_tokens={}", m.input_tokens));
    lines.push(format!("output_tokens={}", m.output_tokens));
    lines.push(format!("project_id={}", escape_value(&m.project_id)));
    lines.push(format!("agent={}", escape_value(&m.agent)));
    lines.push(format!("runtime={}", escape_value(&m.runtime)));
    lines.push(format!("workspace={}", escape_value(&m.workspace)));
    lines.push(format!("tracker={}", escape_value(&m.tracker)));
    lines.join("\n") + "\n"
}

fn deserialize_metadata(content: &str) -> Result<SessionMetadata, StoreError> {
    let mut map: HashMap<String, String> = HashMap::new();
    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        let idx = line
            .find('=')
            .ok_or_else(|| StoreError::Serialization(format!("malformed metadata line: {line}")))?;
        let key = line[..idx].to_string();
        let value = unescape_value(&line[idx + 1..]);
        map.insert(key, value);
    }

    let get = |key: &str| -> Result<String, StoreError> {
        map.get(key)
            .cloned()
            .ok_or_else(|| StoreError::Serialization(format!("missing field: {key}")))
    };

    let id = get("id")?;
    let status_str = get("status")?;
    let status = SessionStatus::from_str(&status_str)
        .map_err(|e| StoreError::Serialization(format!("invalid status '{status_str}': {e}")))?;

    let termination_reason_str = get("termination_reason")?;
    let termination_reason = if termination_reason_str.is_empty() {
        None
    } else {
        Some(
            TerminationReason::from_str(&termination_reason_str).map_err(|e| {
                StoreError::Serialization(format!(
                    "invalid termination_reason '{termination_reason_str}': {e}"
                ))
            })?,
        )
    };

    let issue_url = get("issue_url")?;
    let branch = get("branch")?;
    let worktree_path = PathBuf::from(get("worktree_path")?);

    let created_at = get("created_at")?
        .parse::<u64>()
        .map_err(|e| StoreError::Serialization(format!("invalid created_at: {e}")))?;
    let updated_at = get("updated_at")?
        .parse::<u64>()
        .map_err(|e| StoreError::Serialization(format!("invalid updated_at: {e}")))?;
    let attempts = get("attempts")?
        .parse::<u32>()
        .map_err(|e| StoreError::Serialization(format!("invalid attempts: {e}")))?;
    let total_cost_usd = get("total_cost_usd")?
        .parse::<f64>()
        .map_err(|e| StoreError::Serialization(format!("invalid total_cost_usd: {e}")))?;
    let input_tokens = get("input_tokens")?
        .parse::<u64>()
        .map_err(|e| StoreError::Serialization(format!("invalid input_tokens: {e}")))?;
    let output_tokens = get("output_tokens")?
        .parse::<u64>()
        .map_err(|e| StoreError::Serialization(format!("invalid output_tokens: {e}")))?;
    let project_id = get("project_id")?;
    let agent = get("agent")?;
    let runtime = get("runtime")?;
    let workspace = get("workspace")?;
    let tracker = get("tracker")?;

    Ok(SessionMetadata {
        id,
        status,
        termination_reason,
        issue_url,
        branch,
        worktree_path,
        created_at,
        updated_at,
        attempts,
        total_cost_usd,
        input_tokens,
        output_tokens,
        project_id,
        agent,
        runtime,
        workspace,
        tracker,
    })
}

pub struct SessionStore {
    paths: DataPaths,
}

impl SessionStore {
    pub fn new(paths: DataPaths) -> Self {
        Self { paths }
    }

    /// Create a new session directory (fails if already exists).
    pub async fn create_session(&self, metadata: &SessionMetadata) -> Result<(), StoreError> {
        let session_dir = self.paths.session_dir(&metadata.id);
        match fs::create_dir(&session_dir).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(StoreError::AlreadyExists(metadata.id.clone()));
            }
            Err(e) => return Err(StoreError::Io(e)),
        }
        self.write_metadata(metadata).await
    }

    /// Read session metadata from disk.
    pub async fn read_metadata(&self, session_id: &str) -> Result<SessionMetadata, StoreError> {
        let path = self.paths.metadata_file(session_id);
        let content = fs::read_to_string(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StoreError::NotFound(session_id.to_string())
            } else {
                StoreError::Io(e)
            }
        })?;
        deserialize_metadata(&content)
    }

    /// Write session metadata atomically (tmp → sync → rename).
    pub async fn write_metadata(&self, metadata: &SessionMetadata) -> Result<(), StoreError> {
        let path = self.paths.metadata_file(&metadata.id);
        let tmp_path = path.with_extension("tmp");
        let content = serialize_metadata(metadata);
        let mut file = fs::File::create(&tmp_path).await?;
        file.write_all(content.as_bytes()).await?;
        file.sync_all().await?;
        drop(file);
        fs::rename(&tmp_path, &path).await?;
        Ok(())
    }

    /// Append a journal entry (JSONL, sync after write).
    pub async fn append_journal(
        &self,
        session_id: &str,
        entry: &JournalEntry,
    ) -> Result<(), StoreError> {
        let path = self.paths.journal_file(session_id);
        let line =
            serde_json::to_string(entry).map_err(|e| StoreError::Serialization(e.to_string()))?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        file.write_all(format!("{line}\n").as_bytes()).await?;
        file.sync_all().await?;
        Ok(())
    }

    /// Read all valid journal entries (skip malformed trailing line).
    pub async fn read_journal(&self, session_id: &str) -> Result<Vec<JournalEntry>, StoreError> {
        let path = self.paths.journal_file(session_id);
        let content = match fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(StoreError::Io(e)),
        };

        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

        if lines.is_empty() {
            return Ok(vec![]);
        }

        let last_idx = lines.len() - 1;
        let mut entries = Vec::with_capacity(lines.len());

        for (i, line) in lines.iter().enumerate() {
            match serde_json::from_str::<JournalEntry>(line) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    if i == last_idx {
                        tracing::warn!(
                            session_id,
                            line = *line,
                            error = %e,
                            "skipping malformed trailing journal line"
                        );
                    } else {
                        return Err(StoreError::Serialization(format!(
                            "malformed journal entry at line {}: {e}",
                            i + 1
                        )));
                    }
                }
            }
        }

        Ok(entries)
    }

    /// List all sessions by reading every metadata file under sessions_dir.
    /// Sessions with unreadable/malformed metadata are silently skipped.
    pub async fn list(&self) -> Result<Vec<SessionMetadata>, StoreError> {
        let sessions_dir = self.paths.sessions_dir();
        let mut sessions = Vec::new();

        let mut read_dir = match fs::read_dir(&sessions_dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(StoreError::Io(e)),
        };

        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let session_id = match path.file_name().and_then(|n| n.to_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };
            match self.read_metadata(&session_id).await {
                Ok(meta) => sessions.push(meta),
                Err(e) => {
                    tracing::warn!(session_id = %session_id, error = %e, "skipping unreadable session");
                }
            }
        }

        Ok(sessions)
    }

    /// List only non-terminal sessions.
    pub async fn list_active(&self) -> Result<Vec<SessionMetadata>, StoreError> {
        let all = self.list().await?;
        Ok(all
            .into_iter()
            .filter(|s| !s.status.is_terminal())
            .collect())
    }

    /// Delete a session directory recursively.
    pub async fn delete_session(&self, session_id: &str) -> Result<(), StoreError> {
        let session_dir = self.paths.session_dir(session_id);
        // Attempt removal directly; map NotFound to StoreError::NotFound to
        // avoid a TOCTOU race between an exists() check and remove_dir_all().
        match fs::remove_dir_all(&session_dir).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StoreError::NotFound(session_id.to_string()))
            }
            Err(e) => Err(StoreError::Io(e)),
        }
    }

    /// Return a reference to the underlying DataPaths.
    pub fn paths_ref(&self) -> &DataPaths {
        &self.paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_metadata(id: &str) -> SessionMetadata {
        SessionMetadata {
            id: id.to_string(),
            status: SessionStatus::Working,
            termination_reason: None,
            issue_url: "https://github.com/org/repo/issues/1".to_string(),
            branch: "feat/fix-bug".to_string(),
            worktree_path: PathBuf::from("/home/user/.agent-orchestrator/abc123/worktrees/sess"),
            created_at: 1700000000,
            updated_at: 1700000001,
            attempts: 1,
            total_cost_usd: 0.0123,
            input_tokens: 1000,
            output_tokens: 500,
            project_id: "abc123".to_string(),
            agent: "claude-code".to_string(),
            runtime: "tmux".to_string(),
            workspace: "worktree".to_string(),
            tracker: "github".to_string(),
        }
    }

    fn make_store(root: &std::path::Path) -> SessionStore {
        // Create sessions dir ahead of time so create_session can atomically create child dirs.
        let paths = DataPaths::from_root(root.to_path_buf());
        std::fs::create_dir_all(paths.sessions_dir()).unwrap();
        SessionStore::new(paths)
    }

    #[tokio::test]
    async fn test_create_session_roundtrip() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let meta = make_metadata("myproj-42-1");

        store.create_session(&meta).await.unwrap();
        let read_back = store.read_metadata("myproj-42-1").await.unwrap();

        assert_eq!(read_back.id, meta.id);
        assert_eq!(read_back.status, meta.status);
        assert_eq!(read_back.termination_reason, meta.termination_reason);
        assert_eq!(read_back.issue_url, meta.issue_url);
        assert_eq!(read_back.branch, meta.branch);
        assert_eq!(read_back.worktree_path, meta.worktree_path);
        assert_eq!(read_back.created_at, meta.created_at);
        assert_eq!(read_back.updated_at, meta.updated_at);
        assert_eq!(read_back.attempts, meta.attempts);
        assert!((read_back.total_cost_usd - meta.total_cost_usd).abs() < f64::EPSILON);
        assert_eq!(read_back.input_tokens, meta.input_tokens);
        assert_eq!(read_back.output_tokens, meta.output_tokens);
        assert_eq!(read_back.project_id, meta.project_id);
        assert_eq!(read_back.agent, meta.agent);
        assert_eq!(read_back.runtime, meta.runtime);
        assert_eq!(read_back.workspace, meta.workspace);
        assert_eq!(read_back.tracker, meta.tracker);
    }

    #[tokio::test]
    async fn test_create_session_duplicate_fails() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let meta = make_metadata("myproj-42-1");

        store.create_session(&meta).await.unwrap();
        let err = store.create_session(&meta).await.unwrap_err();

        assert!(
            matches!(err, StoreError::AlreadyExists(ref id) if id == "myproj-42-1"),
            "expected AlreadyExists, got {err}"
        );
    }

    #[tokio::test]
    async fn test_atomic_write_visible_after() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let mut meta = make_metadata("myproj-42-1");

        store.create_session(&meta).await.unwrap();

        meta.status = SessionStatus::PrOpen;
        meta.updated_at = 1700000099;
        store.write_metadata(&meta).await.unwrap();

        let path = store.paths.metadata_file("myproj-42-1");
        assert!(path.exists(), "metadata file should exist at {path:?}");

        let read_back = store.read_metadata("myproj-42-1").await.unwrap();
        assert_eq!(read_back.status, SessionStatus::PrOpen);
        assert_eq!(read_back.updated_at, 1700000099);
    }

    #[tokio::test]
    async fn test_journal_append_and_read() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let meta = make_metadata("myproj-42-1");
        store.create_session(&meta).await.unwrap();

        let entries = vec![
            JournalEntry {
                timestamp: 1700000001,
                event: "spawned".to_string(),
                data: HashMap::from([("key".to_string(), serde_json::json!("value1"))]),
            },
            JournalEntry {
                timestamp: 1700000002,
                event: "working".to_string(),
                data: HashMap::from([("turns".to_string(), serde_json::json!(3))]),
            },
            JournalEntry {
                timestamp: 1700000003,
                event: "done".to_string(),
                data: HashMap::new(),
            },
        ];

        for entry in &entries {
            store.append_journal("myproj-42-1", entry).await.unwrap();
        }

        let read_back = store.read_journal("myproj-42-1").await.unwrap();
        assert_eq!(read_back.len(), 3);
        assert_eq!(read_back[0].event, "spawned");
        assert_eq!(read_back[0].timestamp, 1700000001);
        assert_eq!(read_back[1].event, "working");
        assert_eq!(read_back[2].event, "done");
    }

    #[tokio::test]
    async fn test_journal_malformed_trailing_line() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let meta = make_metadata("myproj-42-1");
        store.create_session(&meta).await.unwrap();

        let valid_entry = JournalEntry {
            timestamp: 1700000001,
            event: "spawned".to_string(),
            data: HashMap::new(),
        };
        store
            .append_journal("myproj-42-1", &valid_entry)
            .await
            .unwrap();

        // Manually append a malformed JSON line to the journal file
        let journal_path = store.paths.journal_file("myproj-42-1");
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&journal_path)
            .await
            .unwrap();
        file.write_all(b"this is not valid json\n").await.unwrap();
        file.sync_all().await.unwrap();

        let read_back = store.read_journal("myproj-42-1").await.unwrap();
        assert_eq!(
            read_back.len(),
            1,
            "malformed trailing line should be skipped"
        );
        assert_eq!(read_back[0].event, "spawned");
    }

    #[tokio::test]
    async fn test_list_returns_all_sessions() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());

        let m1 = make_metadata("sess-1");
        let m2 = make_metadata("sess-2");
        store.create_session(&m1).await.unwrap();
        store.create_session(&m2).await.unwrap();

        let sessions = store.list().await.unwrap();
        assert_eq!(sessions.len(), 2);
        let ids: Vec<_> = sessions.iter().map(|s| s.id.clone()).collect();
        assert!(ids.contains(&"sess-1".to_string()));
        assert!(ids.contains(&"sess-2".to_string()));
    }

    #[tokio::test]
    async fn test_list_skips_malformed_metadata() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());

        let m = make_metadata("good-sess");
        store.create_session(&m).await.unwrap();

        // Create a session dir with no metadata file (simulates partial write)
        let bad_dir = store.paths.session_dir("bad-sess");
        std::fs::create_dir(&bad_dir).unwrap();

        let sessions = store.list().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "good-sess");
    }

    #[tokio::test]
    async fn test_list_empty_store() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let sessions = store.list().await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_list_active_filters_terminal() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());

        let mut m1 = make_metadata("active-sess");
        m1.status = SessionStatus::Working;
        let mut m2 = make_metadata("dead-sess");
        m2.status = SessionStatus::Killed;

        store.create_session(&m1).await.unwrap();
        store.create_session(&m2).await.unwrap();

        let active = store.list_active().await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "active-sess");
    }

    #[tokio::test]
    async fn test_delete_session() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let m = make_metadata("to-delete");
        store.create_session(&m).await.unwrap();

        store.delete_session("to-delete").await.unwrap();
        assert!(matches!(
            store.read_metadata("to-delete").await,
            Err(StoreError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_delete_session_not_found() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        assert!(matches!(
            store.delete_session("nonexistent").await,
            Err(StoreError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_paths_ref() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let paths = store.paths_ref();
        assert!(paths.sessions_dir().exists());
    }
}
