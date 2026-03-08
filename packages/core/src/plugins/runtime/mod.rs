pub mod tmux;

use async_trait::async_trait;
use crate::types::RuntimeStep;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("step not supported: {0}")]
    StepNotSupported(String),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("runtime not alive")]
    NotAlive,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait Runtime: Send + Sync {
    /// Execute a single step from a LaunchPlan.
    async fn execute_step(
        &self,
        session_id: &str,
        step: &RuntimeStep,
    ) -> Result<(), RuntimeError>;
    /// Capture current pane output.
    async fn get_output(&self, session_id: &str) -> Result<String, RuntimeError>;
    /// Check if the session is still running.
    async fn is_alive(&self, session_id: &str) -> bool;
    /// Destroy the session.
    async fn destroy(&self, session_id: &str) -> Result<(), RuntimeError>;
    /// Return the step types this runtime supports.
    fn supported_steps(&self) -> Vec<&'static str>;
}
