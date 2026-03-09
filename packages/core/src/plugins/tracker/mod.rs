use async_trait::async_trait;
use thiserror::Error;

pub mod github;

pub use github::GitHubTracker;

#[derive(Debug, Error)]
pub enum TrackerError {
    #[error("issue not found: {0}")]
    NotFound(String),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("validation failed: {0}")]
    ValidationFailed(String),
}

/// Structured issue content for prompt rendering.
#[derive(Debug, Clone)]
pub struct IssueContent {
    pub title: String,
    pub body: String,
    pub comments: Vec<IssueComment>,
    pub state: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub author: String,
    pub number: u64,
}

#[derive(Debug, Clone)]
pub struct IssueComment {
    pub author: String,
    pub body: String,
    pub created_at: String,
}

/// Tracker state classification (not pluggable).
#[derive(Debug, Clone, PartialEq)]
pub enum TrackerState {
    Active,
    Terminal,
}

/// Classifies a GitHub issue state into TrackerState.
/// Unmatched states default to Active.
pub fn classify_state(state: &str, terminal_states: &[String]) -> TrackerState {
    if terminal_states
        .iter()
        .any(|s| s.eq_ignore_ascii_case(state))
    {
        TrackerState::Terminal
    } else {
        TrackerState::Active
    }
}

#[async_trait]
pub trait Tracker: Send + Sync {
    /// Fetch raw issue JSON. Returns None if not found.
    async fn get_issue(
        &self,
        issue_url: &str,
    ) -> Result<Option<serde_json::Value>, TrackerError>;

    /// Derive the branch name for the issue (e.g. "123-fix-bug").
    fn branch_name(&self, issue_number: u64, title: &str) -> String;

    /// Return the canonical issue URL.
    fn issue_url(&self, issue_number: u64) -> String;

    /// Fetch structured issue content for prompt rendering.
    async fn get_issue_content(&self, issue_url: &str) -> Result<IssueContent, TrackerError>;

    /// Post a comment on the issue.
    async fn add_comment(&self, issue_url: &str, body: &str) -> Result<(), TrackerError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_state_terminal() {
        let result = classify_state("closed", &["closed".to_string()]);
        assert_eq!(result, TrackerState::Terminal);
    }

    #[test]
    fn test_classify_state_active_default() {
        let result = classify_state("unknown_state", &["closed".to_string()]);
        assert_eq!(result, TrackerState::Active);
    }

    #[test]
    fn test_classify_state_case_insensitive() {
        let result = classify_state("CLOSED", &["closed".to_string()]);
        assert_eq!(result, TrackerState::Terminal);
    }
}
