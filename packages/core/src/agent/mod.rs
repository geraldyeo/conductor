mod claude_code;
pub use claude_code::ClaudeCodeAgent;

use crate::types::{LaunchPlan, PluginMeta};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub session_id: String,
    pub prompt: String,
    pub workspace_path: PathBuf,
    pub issue_id: String,
    pub branch: String,
}

#[derive(Debug, Clone)]
pub struct GatherContext {
    pub terminal_output: String,
    pub auxiliary_log: Option<String>,
    pub auxiliary_log_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityState {
    Active,
    Ready,
    Idle,
    WaitingInput,
    Blocked,
    Exited,
}

pub trait Agent: Send + Sync {
    fn meta(&self) -> PluginMeta;
    fn launch_plan(&self, ctx: &LaunchContext) -> LaunchPlan;
    fn detect_activity(&self, ctx: &GatherContext) -> ActivityState;
    fn auxiliary_log_path(&self) -> Option<PathBuf> {
        None
    }
}

pub fn create_agent(name: &str) -> Result<Box<dyn Agent>, crate::types::PluginError> {
    match name {
        "claude-code" | "claude" => Ok(Box::new(ClaudeCodeAgent::new())),
        other => Err(crate::types::PluginError::NotImplemented(other.into())),
    }
}
