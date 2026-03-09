use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorRequest {
    Spawn {
        project_id: String,
        issue_id: String,
    },
    Stop,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorResponse {
    SpawnResult {
        session_id: String,
        branch: String,
    },
    Ok {
        message: String,
    },
    Error {
        code: String,
        message: String,
    },
}

/// Returns the path to the orchestrator Unix domain socket.
pub fn socket_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home)
        .join(".agent-orchestrator")
        .join("orchestrator.sock")
}

/// Send a length-prefixed JSON request to the orchestrator over a Unix domain socket.
pub async fn send_request(
    request: &OrchestratorRequest,
) -> Result<OrchestratorResponse, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let path = socket_path();
    let mut stream = UnixStream::connect(&path).await.map_err(|e| {
        format!(
            "cannot connect to orchestrator at {}: {e}\nRun `ao start` first.",
            path.display()
        )
    })?;

    let payload = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len).await.map_err(|e| e.to_string())?;
    stream
        .write_all(&payload)
        .await
        .map_err(|e| e.to_string())?;

    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| e.to_string())?;
    let resp_len = u32::from_be_bytes(len_buf) as usize;
    let mut resp_buf = vec![0u8; resp_len];
    stream
        .read_exact(&mut resp_buf)
        .await
        .map_err(|e| e.to_string())?;

    serde_json::from_slice(&resp_buf).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_path_under_home() {
        let path = socket_path();
        assert!(
            path.to_string_lossy().contains(".agent-orchestrator"),
            "socket path should be under .agent-orchestrator: {}",
            path.display()
        );
        assert!(
            path.to_string_lossy().ends_with("orchestrator.sock"),
            "socket should be named orchestrator.sock: {}",
            path.display()
        );
    }

    #[test]
    fn test_spawn_request_serializes() {
        let req = OrchestratorRequest::Spawn {
            project_id: "proj-1".to_string(),
            issue_id: "42".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"spawn\""));
        assert!(json.contains("\"issue_id\":\"42\""));
    }

    #[test]
    fn test_stop_request_serializes() {
        let req = OrchestratorRequest::Stop;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"stop\""));
    }

    #[test]
    fn test_spawn_result_deserializes() {
        let json = r#"{"type":"spawn_result","session_id":"ao-abc123","branch":"feat/test"}"#;
        let resp: OrchestratorResponse = serde_json::from_str(json).unwrap();
        match resp {
            OrchestratorResponse::SpawnResult {
                session_id,
                branch,
            } => {
                assert_eq!(session_id, "ao-abc123");
                assert_eq!(branch, "feat/test");
            }
            other => panic!("expected SpawnResult, got: {other:?}"),
        }
    }

    #[test]
    fn test_error_response_deserializes() {
        let json =
            r#"{"type":"error","code":"session_exists","message":"already running"}"#;
        let resp: OrchestratorResponse = serde_json::from_str(json).unwrap();
        match resp {
            OrchestratorResponse::Error { code, message } => {
                assert_eq!(code, "session_exists");
                assert_eq!(message, "already running");
            }
            other => panic!("expected Error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_send_request_fails_when_no_orchestrator() {
        let req = OrchestratorRequest::Spawn {
            project_id: "test".to_string(),
            issue_id: "1".to_string(),
        };
        let result = send_request(&req).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("cannot connect to orchestrator"),
            "expected connection error, got: {err}"
        );
    }
}
