mod plugin;
mod runtime;
mod status;

pub use plugin::{PluginError, PluginMeta};
pub use runtime::{LaunchPlan, RuntimeStep};
pub use status::{SessionStatus, TerminationReason};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueContent {
    pub title: String,
    pub body: String,
    pub comments: Vec<IssueComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueComment {
    pub author: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackerState {
    Active,
    Terminal,
}

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
}
