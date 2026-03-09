use crate::error::CliError;
use crate::ipc::client::send_request;
use conductor_core::ipc::{OrchestratorRequest, OrchestratorResponse};
use conductor_core::utils::CommandRunner;

pub async fn run(
    session_id: &str,
    content: &str,
    no_wait: bool,
    timeout: u64,
    json: bool,
) -> Result<(), CliError> {
    let req = OrchestratorRequest::Send {
        session_id: session_id.to_string(),
        content: content.to_string(),
        no_wait,
        timeout_secs: timeout,
    };

    match send_request(&req).await {
        Ok(OrchestratorResponse::SendResult {
            delivered,
            activity_state,
        }) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"delivered": delivered, "activity_state": activity_state})
                );
            } else {
                println!("Delivered: {delivered} (agent state: {activity_state})");
            }
        }
        Err(crate::ipc::client::IpcError::NotRunning(_)) => {
            // Fallback: direct tmux send-keys
            eprintln!("warning: orchestrator not running, delivering without busy detection");
            let runner = CommandRunner;
            runner
                .run(
                    &["tmux", "send-keys", "-t", session_id, content, "Enter"],
                    None,
                    None,
                )
                .await
                .map_err(|e| CliError::General(e.to_string()))?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({"delivered": true, "activity_state": "unknown", "fallback": true})
                );
            } else {
                println!("Delivered via direct tmux send-keys.");
            }
        }
        Err(e) => return Err(CliError::Ipc(e)),
        Ok(other) => return Err(CliError::General(format!("unexpected response: {other:?}"))),
    }

    Ok(())
}
