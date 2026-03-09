use crate::error::CliError;
use crate::ipc::client::send_request;
use conductor_core::ipc::OrchestratorRequest;

pub async fn run(json: bool) -> Result<(), CliError> {
    match send_request(&OrchestratorRequest::Stop).await {
        Ok(_) => {
            if json {
                println!("{}", serde_json::json!({"status": "stopped"}));
            } else {
                println!("Orchestrator stopped.");
            }
        }
        Err(crate::ipc::client::IpcError::NotRunning(_)) => {
            if !json {
                println!("Orchestrator is not running.");
            }
        }
        Err(e) => return Err(CliError::Ipc(e)),
    }
    Ok(())
}
