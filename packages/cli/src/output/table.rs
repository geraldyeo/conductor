use conductor_core::session_store::SessionMetadata;

pub fn format_sessions(sessions: &[SessionMetadata]) -> String {
    if sessions.is_empty() {
        return "No sessions.".to_string();
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let header = format!(
        "{:<25} {:<18} {:<8} {:<28} {:<8} {}",
        "SESSION", "STATUS", "ISSUE", "BRANCH", "AGE", "TOKENS"
    );
    let sep = "-".repeat(header.len());

    let mut rows = vec![header, sep];
    for s in sessions {
        let age = format_age(now.saturating_sub(s.created_at));
        let tokens = format!(
            "{} in / {} out",
            format_tokens(s.input_tokens),
            format_tokens(s.output_tokens)
        );
        let issue = extract_issue_number(&s.issue_url)
            .map(|n| format!("#{n}"))
            .unwrap_or_else(|| s.issue_url.clone());
        rows.push(format!(
            "{:<25} {:<18} {:<8} {:<28} {:<8} {}",
            s.id, s.status, issue, s.branch, age, tokens
        ));
    }
    rows.join("\n")
}

fn format_age(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

fn format_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn extract_issue_number(url: &str) -> Option<u64> {
    conductor_core::utils::parse_github_issue_number(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1.0k");
        assert_eq!(format_tokens(12400), "12.4k");
    }

    #[test]
    fn test_format_age() {
        assert_eq!(format_age(30), "30s");
        assert_eq!(format_age(90), "1m");
        assert_eq!(format_age(7200), "2h");
    }
}
