use conductor_core::ipc::{
    read_message, socket_path, write_message, OrchestratorRequest, OrchestratorResponse,
};
use thiserror::Error;
use tokio::net::UnixStream;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("orchestrator is not running (socket not found at {0:?})")]
    NotRunning(std::path::PathBuf),
    #[error("IPC error: {0}")]
    Io(#[from] std::io::Error),
    #[error("orchestrator returned error: [{code}] {message}")]
    Orchestrator { code: String, message: String },
}

/// Send a request to the running orchestrator and return the response.
/// Returns `IpcError::NotRunning` if the socket doesn't exist or connection is refused.
pub async fn send_request(request: &OrchestratorRequest) -> Result<OrchestratorResponse, IpcError> {
    let path = socket_path();

    if !path.exists() {
        return Err(IpcError::NotRunning(path));
    }

    let mut stream = UnixStream::connect(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused {
            IpcError::NotRunning(path.clone())
        } else {
            IpcError::Io(e)
        }
    })?;

    write_message(&mut stream, request).await?;
    let response: OrchestratorResponse = read_message(&mut stream).await?;

    if let OrchestratorResponse::Error { code, message } = response {
        return Err(IpcError::Orchestrator { code, message });
    }

    Ok(response)
}
