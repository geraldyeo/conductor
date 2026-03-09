use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Returns the path to the orchestrator Unix domain socket.
pub fn socket_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".agent-orchestrator").join("orchestrator.sock")
}

// ── Request / Response types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorRequest {
    Spawn {
        project_id: String,
        issue_url: String,
        agent: Option<String>,
        open: bool,
    },
    BatchSpawn {
        project_id: String,
        issue_urls: Vec<String>,
        agent: Option<String>,
        open: bool,
    },
    Send {
        session_id: String,
        content: String,
        no_wait: bool,
        timeout_secs: u64,
    },
    Kill {
        session_id: String,
    },
    Cleanup {
        project_id: String,
        dry_run: bool,
    },
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorResponse {
    Ok {
        message: String,
    },
    SpawnResult {
        session_id: String,
        branch: String,
        workspace_path: String,
    },
    BatchSpawnResult {
        results: Vec<BatchSpawnItem>,
    },
    CleanupResult {
        killed: Vec<String>,
        skipped: Vec<String>,
    },
    SendResult {
        delivered: bool,
        activity_state: String,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSpawnItem {
    pub issue_url: String,
    pub outcome: BatchSpawnOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum BatchSpawnOutcome {
    Spawned { session_id: String, branch: String },
    Skipped { reason: String },
    Failed { error: String },
}

// ── Length-prefixed JSON framing ─────────────────────────────────────────────
// Wire format: 4-byte big-endian u32 length prefix, then UTF-8 JSON body.

const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024; // 4 MB

pub async fn write_message<T: Serialize, W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    value: &T,
) -> std::io::Result<()> {
    let json = serde_json::to_vec(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if json.len() > MAX_MESSAGE_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("IPC message too large: {} bytes", json.len()),
        ));
    }
    let len = json.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&json).await?;
    writer.flush().await
}

pub async fn read_message<T: for<'de> Deserialize<'de>, R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> std::io::Result<T> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("IPC message too large: {len} bytes"),
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_path_is_absolute() {
        let path = socket_path();
        assert!(path.is_absolute());
    }

    #[test]
    fn test_socket_path_filename() {
        let path = socket_path();
        assert_eq!(
            path.file_name().and_then(|f| f.to_str()),
            Some("orchestrator.sock")
        );
    }

    #[tokio::test]
    async fn test_write_read_roundtrip() {
        let req = OrchestratorRequest::Spawn {
            project_id: "myapp".to_string(),
            issue_url: "https://github.com/org/repo/issues/42".to_string(),
            agent: None,
            open: false,
        };
        let mut buf = Vec::new();
        write_message(&mut buf, &req).await.unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let back: OrchestratorRequest = read_message(&mut cursor).await.unwrap();
        assert!(matches!(back, OrchestratorRequest::Spawn { .. }));
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[test]
    fn test_spawn_request_roundtrip() {
        let req = OrchestratorRequest::Spawn {
            project_id: "myapp".to_string(),
            issue_url: "https://github.com/org/repo/issues/42".to_string(),
            agent: None,
            open: false,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: OrchestratorRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, OrchestratorRequest::Spawn { .. }));
    }

    #[test]
    fn test_spawn_result_response_roundtrip() {
        let resp = OrchestratorResponse::SpawnResult {
            session_id: "ao-abc123".to_string(),
            branch: "feat/issue-42".to_string(),
            workspace_path: "/tmp/worktrees/ao-abc123".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: OrchestratorResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, OrchestratorResponse::SpawnResult { .. }));
    }

    #[test]
    fn test_error_response_roundtrip() {
        let resp = OrchestratorResponse::Error {
            code: "issue_terminal".to_string(),
            message: "issue is closed".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: OrchestratorResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, OrchestratorResponse::Error { .. }));
    }

    #[test]
    fn test_batch_spawn_outcome_roundtrip() {
        let item = BatchSpawnItem {
            issue_url: "https://github.com/org/repo/issues/1".to_string(),
            outcome: BatchSpawnOutcome::Spawned {
                session_id: "sess-1".to_string(),
                branch: "feat/issue-1".to_string(),
            },
        };
        let json = serde_json::to_string(&item).unwrap();
        let back: BatchSpawnItem = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.outcome, BatchSpawnOutcome::Spawned { .. }));
    }
}
