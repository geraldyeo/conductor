pub mod claude_code;

use async_trait::async_trait;
use crate::types::LaunchPlan;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent not supported: {0}")]
    NotSupported(String),
    #[error("parse error: {0}")]
    ParseError(String),
}

/// Context passed to activity detection and session info parsing.
#[derive(Debug, Clone)]
pub struct GatherContext {
    pub session_id: String,
    pub raw_output: String,
    pub last_activity_at: u64,
}

/// Per-session info parsed from agent output.
#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[async_trait]
pub trait Agent: Send + Sync {
    /// Produce a LaunchPlan for the given session and prompt.
    fn launch_plan(&self, session_id: &str, prompt: &str) -> LaunchPlan;
    /// Detect if the agent is active based on pane output.
    fn detect_activity(&self, ctx: &GatherContext) -> bool;
    /// Parse cost/token data from pane output.
    fn parse_session_info(&self, ctx: &GatherContext) -> SessionInfo;
}
