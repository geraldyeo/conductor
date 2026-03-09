use super::{IssueComment, IssueContent, Tracker, TrackerError};
use crate::utils::{parse_github_issue_number, CommandRunner};
use async_trait::async_trait;

pub struct GitHubTracker {
    runner: CommandRunner,
    repo: String, // "owner/repo"
    terminal_states: Vec<String>,
}

impl GitHubTracker {
    /// Factory — calls validate() immediately.
    pub async fn new(
        runner: CommandRunner,
        repo: String,
        terminal_states: Vec<String>,
    ) -> Result<Self, TrackerError> {
        let tracker = Self {
            runner,
            repo,
            terminal_states,
        };
        tracker.validate().await?;
        Ok(tracker)
    }

    /// Fail-fast validation: check that `gh` CLI is available.
    async fn validate(&self) -> Result<(), TrackerError> {
        self.runner
            .run(&["gh", "--version"], None, None)
            .await
            .map_err(|e| TrackerError::ValidationFailed(format!("gh CLI not available: {e}")))?;
        Ok(())
    }

    /// Returns the configured terminal states (used by the orchestrator with classify_state()).
    pub fn terminal_states(&self) -> &[String] {
        &self.terminal_states
    }

    #[cfg(test)]
    fn for_tests(repo: &str) -> Self {
        Self {
            runner: CommandRunner::default(),
            repo: repo.to_string(),
            terminal_states: vec!["closed".to_string()],
        }
    }
}

/// Parse issue number from a GitHub issue URL.
/// Expected format: `https://github.com/owner/repo/issues/123`
fn parse_issue_number(url: &str) -> Result<u64, TrackerError> {
    parse_github_issue_number(url).ok_or_else(|| {
        TrackerError::ParseError(format!("cannot parse issue number from URL: {url}"))
    })
}

/// Parse a `serde_json::Value` (from `gh issue view --json ...`) into `IssueContent`.
pub fn parse_issue_content(value: serde_json::Value) -> Result<IssueContent, TrackerError> {
    let title = value["title"]
        .as_str()
        .ok_or_else(|| TrackerError::ParseError("missing field: title".to_string()))?
        .to_string();

    let body = value["body"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let state = value["state"]
        .as_str()
        .ok_or_else(|| TrackerError::ParseError("missing field: state".to_string()))?
        .to_string();

    let number = value["number"]
        .as_u64()
        .ok_or_else(|| TrackerError::ParseError("missing field: number".to_string()))?;

    let author = value["author"]["login"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let labels = value["labels"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let assignees = value["assignees"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a["login"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let comments = value["comments"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| IssueComment {
                    author: c["author"]["login"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    body: c["body"].as_str().unwrap_or("").to_string(),
                    created_at: c["createdAt"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(IssueContent {
        title,
        body,
        comments,
        state,
        labels,
        assignees,
        author,
        number,
    })
}

#[async_trait]
impl Tracker for GitHubTracker {
    async fn get_issue(
        &self,
        issue_url: &str,
    ) -> Result<Option<serde_json::Value>, TrackerError> {
        let issue_number = parse_issue_number(issue_url)?;
        let output = self
            .runner
            .run(
                &[
                    "gh",
                    "issue",
                    "view",
                    &issue_number.to_string(),
                    "--repo",
                    &self.repo,
                    "--json",
                    "number,title,body,state,labels,assignees,author,comments",
                ],
                None,
                None,
            )
            .await
            .map_err(|e| TrackerError::CommandFailed(e.to_string()))?;

        if !output.success {
            if output.stderr.contains("Could not resolve")
                || output.stderr.contains("not found")
            {
                return Ok(None);
            }
            return Err(TrackerError::CommandFailed(output.stderr));
        }

        let value: serde_json::Value = serde_json::from_str(&output.stdout)
            .map_err(|e| TrackerError::ParseError(e.to_string()))?;
        Ok(Some(value))
    }

    fn branch_name(&self, issue_number: u64, title: &str) -> String {
        // Slugify: lowercase, replace non-alphanumeric with hyphens, collapse runs, strip leading/trailing hyphens
        let slug: String = title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let slug: String = slug.chars().take(50).collect();
        // Trim trailing hyphens from truncation
        let slug = slug.trim_end_matches('-');
        format!("{issue_number}-{slug}")
    }

    fn issue_url(&self, issue_number: u64) -> String {
        format!("https://github.com/{}/issues/{}", self.repo, issue_number)
    }

    async fn get_issue_content(&self, issue_url: &str) -> Result<IssueContent, TrackerError> {
        let value = self
            .get_issue(issue_url)
            .await?
            .ok_or_else(|| TrackerError::NotFound(issue_url.to_string()))?;
        parse_issue_content(value)
    }

    async fn add_comment(&self, issue_url: &str, body: &str) -> Result<(), TrackerError> {
        let issue_number = parse_issue_number(issue_url)?;
        let output = self
            .runner
            .run(
                &[
                    "gh",
                    "issue",
                    "comment",
                    &issue_number.to_string(),
                    "--repo",
                    &self.repo,
                    "--body",
                    body,
                ],
                None,
                None,
            )
            .await
            .map_err(|e| TrackerError::CommandFailed(e.to_string()))?;

        if !output.success {
            return Err(TrackerError::CommandFailed(output.stderr));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::tracker::classify_state;
    use crate::plugins::tracker::TrackerState;

    const FIXTURE_JSON: &str = r#"{
        "number": 42,
        "title": "Fix the important bug",
        "body": "This is the issue body",
        "state": "OPEN",
        "labels": [{"name": "bug"}, {"name": "priority-high"}],
        "assignees": [{"login": "alice"}],
        "author": {"login": "bob"},
        "comments": [
            {"author": {"login": "charlie"}, "body": "Looking into this", "createdAt": "2024-01-01T00:00:00Z"},
            {"author": {"login": "alice"}, "body": "Fixed in branch", "createdAt": "2024-01-02T00:00:00Z"}
        ]
    }"#;

    #[test]
    fn test_branch_name_slugifies_title() {
        let tracker = GitHubTracker::for_tests("owner/repo");
        let result = tracker.branch_name(42, "Fix the important bug!");
        assert_eq!(result, "42-fix-the-important-bug");
    }

    #[test]
    fn test_branch_name_truncates_long_title() {
        let tracker = GitHubTracker::for_tests("owner/repo");
        // Build a title that will produce a slug longer than 50 chars
        let long_title = "This is a very long title that should definitely be truncated by our slug";
        let result = tracker.branch_name(1, long_title);
        // The number prefix is separate; the slug portion should be <= 50 chars with no trailing hyphen
        let slug_part = result.strip_prefix("1-").expect("should have number prefix");
        assert!(slug_part.len() <= 50, "slug is {} chars, expected <= 50", slug_part.len());
        assert!(!slug_part.ends_with('-'), "slug should not end with a hyphen");
    }

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
    fn test_parse_issue_content_from_fixture() {
        let value: serde_json::Value =
            serde_json::from_str(FIXTURE_JSON).expect("fixture JSON must parse");
        let content = parse_issue_content(value).expect("parse_issue_content must succeed");

        assert_eq!(content.title, "Fix the important bug");
        assert_eq!(content.labels.len(), 2);
        assert_eq!(content.assignees.len(), 1);
        assert_eq!(content.assignees[0], "alice");
        assert_eq!(content.comments.len(), 2);
        assert_eq!(content.author, "bob");
        assert_eq!(content.number, 42);
    }

    #[test]
    fn test_parse_issue_number_from_url() {
        let result = parse_issue_number("https://github.com/owner/repo/issues/42");
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_issue_url_format() {
        let tracker = GitHubTracker::for_tests("owner/repo");
        let url = tracker.issue_url(42);
        assert_eq!(url, "https://github.com/owner/repo/issues/42");
    }
}
