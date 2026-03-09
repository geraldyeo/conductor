mod command_runner;
mod data_paths;

pub use command_runner::{CommandError, CommandOutput, CommandRunner};
pub use data_paths::{DataPaths, DataPathsError};

/// Parse a GitHub issue number from a URL.
/// Handles full URLs, plain numbers, and strips fragments (`#issuecomment-N`).
/// Returns `None` if no numeric issue number can be extracted.
pub fn parse_github_issue_number(url: &str) -> Option<u64> {
    url.split('/')
        .next_back()
        .and_then(|s| s.split('#').next())
        .and_then(|s| s.parse::<u64>().ok())
}
