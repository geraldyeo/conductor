mod error;
mod journal;
mod metadata;

pub use error::StoreError;
pub use journal::{JournalEntry, JournalResult};
pub use metadata::SessionMetadata;

use crate::utils::DataPaths;
use tokio::io::AsyncWriteExt;

pub struct SessionStore {
    paths: DataPaths,
}

impl SessionStore {
    pub fn new(paths: DataPaths) -> Self {
        Self { paths }
    }

    /// Race-free creation: create_dir fails if session already exists.
    pub async fn create(&self, initial: &SessionMetadata) -> Result<(), StoreError> {
        let dir = self.paths.session_dir(&initial.session_id);
        // Ensure the parent sessions/ dir exists
        tokio::fs::create_dir_all(self.paths.sessions_dir()).await?;
        tokio::fs::create_dir(&dir).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                StoreError::AlreadyExists(initial.session_id.clone())
            } else {
                StoreError::Io(e)
            }
        })?;
        self.write_atomic(&initial.session_id, initial).await
    }

    pub async fn read(&self, session_id: &str) -> Result<SessionMetadata, StoreError> {
        let path = self.paths.metadata_file(session_id);
        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StoreError::NotFound(session_id.to_string())
            } else {
                StoreError::Io(e)
            }
        })?;
        SessionMetadata::deserialize(&content).map_err(StoreError::Parse)
    }

    pub async fn write(
        &self,
        session_id: &str,
        metadata: &SessionMetadata,
    ) -> Result<(), StoreError> {
        self.write_atomic(session_id, metadata).await
    }

    async fn write_atomic(
        &self,
        session_id: &str,
        metadata: &SessionMetadata,
    ) -> Result<(), StoreError> {
        let path = self.paths.metadata_file(session_id);
        let tmp = path.with_extension("tmp");
        let content = metadata.serialize();
        let mut file = tokio::fs::File::create(&tmp).await?;
        file.write_all(content.as_bytes()).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    pub async fn append_journal(
        &self,
        session_id: &str,
        entry: &JournalEntry,
    ) -> Result<(), StoreError> {
        let path = self.paths.journal_file(session_id);
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.sync_all().await?;
        Ok(())
    }

    pub async fn read_journal(
        &self,
        session_id: &str,
    ) -> Result<Vec<JournalEntry>, StoreError> {
        let path = self.paths.journal_file(session_id);
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(StoreError::Io(e)),
        };
        let mut entries = Vec::new();
        for (i, line) in content.lines().enumerate() {
            match serde_json::from_str::<JournalEntry>(line) {
                Ok(e) => entries.push(e),
                Err(err) => tracing::warn!("skipping malformed journal line {i}: {err}"),
            }
        }
        Ok(entries)
    }

    pub async fn list(&self) -> Result<Vec<SessionMetadata>, StoreError> {
        let dir = self.paths.sessions_dir();
        let mut entries = Vec::new();
        let mut read_dir = match tokio::fs::read_dir(&dir).await {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(StoreError::Io(e)),
        };
        while let Some(entry) = read_dir.next_entry().await? {
            let id = entry.file_name().to_string_lossy().into_owned();
            match self.read(&id).await {
                Ok(meta) => entries.push(meta),
                Err(e) => tracing::warn!("skipping malformed session {id}: {e}"),
            }
        }
        Ok(entries)
    }

    pub async fn exists(&self, session_id: &str) -> bool {
        self.paths.metadata_file(session_id).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionStatus;
    use tempfile::tempdir;

    fn make_store(dir: &std::path::Path) -> SessionStore {
        let paths = DataPaths::from_root(dir.to_path_buf());
        SessionStore::new(paths)
    }

    fn sample_metadata(id: &str) -> SessionMetadata {
        SessionMetadata {
            session_id: id.to_string(),
            status: SessionStatus::Spawning,
            created_at: "2026-03-08T00:00:00Z".to_string(),
            updated_at: "2026-03-08T00:00:00Z".to_string(),
            workspace_path: std::path::PathBuf::from("/tmp/ws"),
            agent: "claude-code".to_string(),
            runtime: "tmux".to_string(),
            issue_id: "42".to_string(),
            attempt: 1,
            branch: String::new(),
            base_branch: "main".to_string(),
            pr_url: String::new(),
            tokens_in: 0,
            tokens_out: 0,
            termination_reason: None,
            kill_requested: false,
            tracker_cleanup_requested: false,
        }
    }

    #[tokio::test]
    async fn test_create_and_read() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let meta = sample_metadata("proj-42-1");
        store.create(&meta).await.unwrap();
        let read = store.read("proj-42-1").await.unwrap();
        assert_eq!(read.session_id, "proj-42-1");
        assert_eq!(read.status, SessionStatus::Spawning);
    }

    #[tokio::test]
    async fn test_write_updates_metadata() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let mut meta = sample_metadata("proj-42-1");
        store.create(&meta).await.unwrap();
        meta.status = SessionStatus::Working;
        store.write("proj-42-1", &meta).await.unwrap();
        let read = store.read("proj-42-1").await.unwrap();
        assert_eq!(read.status, SessionStatus::Working);
    }

    #[tokio::test]
    async fn test_create_is_race_free() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        let meta = sample_metadata("proj-42-1");
        store.create(&meta).await.unwrap();
        // Second create on same ID must fail
        let result = store.create(&meta).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_append_and_read_journal() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        store.create(&sample_metadata("proj-42-1")).await.unwrap();
        let entry = JournalEntry {
            action: "spawn".to_string(),
            target: "proj-42-1".to_string(),
            timestamp: "2026-03-08T00:00:00Z".to_string(),
            dedupe_key: "spawn:proj-42-1:1".to_string(),
            result: JournalResult::Success,
            error_code: None,
            attempt: 1,
            actor: "orchestrator".to_string(),
        };
        store.append_journal("proj-42-1", &entry).await.unwrap();
        let entries = store.read_journal("proj-42-1").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action, "spawn");
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        store.create(&sample_metadata("proj-42-1")).await.unwrap();
        let mut meta2 = sample_metadata("proj-43-1");
        meta2.issue_id = "43".to_string();
        store.create(&meta2).await.unwrap();
        let sessions = store.list().await.unwrap();
        assert_eq!(sessions.len(), 2);
    }
}
