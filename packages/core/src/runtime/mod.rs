mod error;
mod tmux;

pub use error::RuntimeError;
pub use tmux::TmuxRuntime;

use async_trait::async_trait;
use crate::types::{PluginMeta, RuntimeStep};

#[async_trait]
pub trait Runtime: Send + Sync {
    fn meta(&self) -> PluginMeta;
    async fn execute_step(&self, session_id: &str, step: &RuntimeStep)
        -> Result<(), RuntimeError>;
    async fn get_output(&self, session_id: &str, lines: usize) -> Result<String, RuntimeError>;
    async fn is_alive(&self, session_id: &str) -> Result<bool, RuntimeError>;
    async fn destroy(&self, session_id: &str) -> Result<(), RuntimeError>;
    fn supported_steps(&self) -> &'static [&'static str] {
        &["create", "wait_for_ready", "send_message", "send_buffer"]
    }
}

pub fn create_runtime(
    name: &str,
    runner: std::sync::Arc<crate::utils::CommandRunner>,
) -> Result<Box<dyn Runtime>, crate::types::PluginError> {
    match name {
        "tmux" => Ok(Box::new(TmuxRuntime::new(runner))),
        other => Err(crate::types::PluginError::NotImplemented(other.into())),
    }
}
