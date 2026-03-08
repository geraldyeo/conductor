use super::{Runtime, RuntimeError};
use crate::types::RuntimeStep;
use crate::utils::CommandRunner;
use async_trait::async_trait;

pub struct TmuxRuntime {
    runner: CommandRunner,
}

impl TmuxRuntime {
    pub fn new(runner: CommandRunner) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl Runtime for TmuxRuntime {
    async fn execute_step(
        &self,
        session_id: &str,
        step: &RuntimeStep,
    ) -> Result<(), RuntimeError> {
        match step {
            RuntimeStep::Create {
                command,
                env: _,
                working_dir,
            } => {
                let args_ref: Vec<&str> = command.iter().map(|s| s.as_str()).collect();
                let out = self
                    .runner
                    .run_in_dir(&args_ref, working_dir, None, None)
                    .await
                    .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
                if !out.success {
                    return Err(RuntimeError::CommandFailed(out.stderr));
                }
                Ok(())
            }
            RuntimeStep::WaitForReady { timeout } => {
                tokio::time::sleep(*timeout).await;
                Ok(())
            }
            RuntimeStep::SendBuffer { content } => {
                let out = self
                    .runner
                    .run(
                        &["tmux", "send-keys", "-t", session_id, content, ""],
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
                if !out.success {
                    return Err(RuntimeError::CommandFailed(out.stderr));
                }
                Ok(())
            }
            RuntimeStep::SendMessage { content } => {
                let out = self
                    .runner
                    .run(
                        &["tmux", "send-keys", "-t", session_id, content, "Enter"],
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
                if !out.success {
                    return Err(RuntimeError::CommandFailed(out.stderr));
                }
                Ok(())
            }
            RuntimeStep::SendProtocol { .. } => {
                Err(RuntimeError::StepNotSupported("SendProtocol".to_string()))
            }
        }
    }

    async fn get_output(&self, session_id: &str) -> Result<String, RuntimeError> {
        let out = self
            .runner
            .run(
                &["tmux", "capture-pane", "-p", "-t", session_id],
                None,
                None,
            )
            .await
            .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
        if !out.success {
            return Err(RuntimeError::CommandFailed(out.stderr));
        }
        Ok(out.stdout)
    }

    async fn is_alive(&self, session_id: &str) -> bool {
        self.runner
            .run(&["tmux", "has-session", "-t", session_id], None, None)
            .await
            .map(|o| o.success)
            .unwrap_or(false)
    }

    async fn destroy(&self, session_id: &str) -> Result<(), RuntimeError> {
        let out = self
            .runner
            .run(&["tmux", "kill-session", "-t", session_id], None, None)
            .await
            .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
        if !out.success {
            return Err(RuntimeError::CommandFailed(out.stderr));
        }
        Ok(())
    }

    fn supported_steps(&self) -> Vec<&'static str> {
        vec!["Create", "WaitForReady", "SendBuffer", "SendMessage"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RuntimeStep;

    fn make_runtime() -> TmuxRuntime {
        TmuxRuntime::new(CommandRunner::new())
    }

    #[test]
    fn test_supported_steps() {
        let runtime = make_runtime();
        let steps = runtime.supported_steps();
        assert!(steps.contains(&"Create"));
        assert!(steps.contains(&"WaitForReady"));
        assert!(steps.contains(&"SendBuffer"));
        assert!(steps.contains(&"SendMessage"));
        assert!(!steps.contains(&"SendProtocol"));
    }

    #[tokio::test]
    async fn test_send_protocol_returns_not_supported() {
        let runtime = make_runtime();
        let step = RuntimeStep::SendProtocol { payload: vec![] };
        let result = runtime.execute_step("test-session", &step).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RuntimeError::StepNotSupported(_)
        ));
    }
}
