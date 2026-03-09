use crate::types::{SessionStatus, TerminationReason};
use std::{path::PathBuf, str::FromStr};

#[derive(Debug, Clone)]
pub struct SessionMetadata {
    pub session_id: String,
    pub status: SessionStatus,
    pub created_at: String,
    pub updated_at: String,
    pub workspace_path: PathBuf,
    pub agent: String,
    pub runtime: String,
    pub issue_id: String,
    pub attempt: u32,
    pub branch: String,
    pub base_branch: String,
    pub pr_url: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub termination_reason: Option<TerminationReason>,
    pub kill_requested: bool,
    pub tracker_cleanup_requested: bool,
}

fn escape_value(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape_value(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('\\') => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

impl SessionMetadata {
    pub fn serialize(&self) -> String {
        let tr = self
            .termination_reason
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_default();
        format!(
            "SESSION_ID={}\nSTATUS={}\nCREATED_AT={}\nUPDATED_AT={}\n\
             WORKSPACE_PATH={}\nAGENT={}\nRUNTIME={}\nISSUE_ID={}\n\
             ATTEMPT={}\nBRANCH={}\nBASE_BRANCH={}\nPR_URL={}\n\
             TOKENS_IN={}\nTOKENS_OUT={}\nTERMINATION_REASON={}\n\
             KILL_REQUESTED={}\nTRACKER_CLEANUP_REQUESTED={}\n",
            escape_value(&self.session_id),
            escape_value(&self.status.to_string()),
            escape_value(&self.created_at),
            escape_value(&self.updated_at),
            escape_value(&self.workspace_path.to_string_lossy()),
            escape_value(&self.agent),
            escape_value(&self.runtime),
            escape_value(&self.issue_id),
            self.attempt,
            escape_value(&self.branch),
            escape_value(&self.base_branch),
            escape_value(&self.pr_url),
            self.tokens_in,
            self.tokens_out,
            escape_value(&tr),
            self.kill_requested,
            self.tracker_cleanup_requested,
        )
    }

    pub fn deserialize(s: &str) -> Result<Self, String> {
        let mut map = std::collections::HashMap::new();
        for line in s.lines() {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.to_string(), unescape_value(v));
            }
        }
        let get = |key: &str| -> Result<String, String> {
            map.get(key)
                .cloned()
                .ok_or_else(|| format!("missing key: {key}"))
        };
        Ok(SessionMetadata {
            session_id: get("SESSION_ID")?,
            status: SessionStatus::from_str(&get("STATUS")?)
                .map_err(|e| format!("bad STATUS: {e}"))?,
            created_at: get("CREATED_AT")?,
            updated_at: get("UPDATED_AT")?,
            workspace_path: PathBuf::from(get("WORKSPACE_PATH")?),
            agent: get("AGENT")?,
            runtime: get("RUNTIME")?,
            issue_id: get("ISSUE_ID")?,
            attempt: get("ATTEMPT")?
                .parse()
                .map_err(|e| format!("bad ATTEMPT: {e}"))?,
            branch: get("BRANCH")?,
            base_branch: get("BASE_BRANCH")?,
            pr_url: get("PR_URL")?,
            tokens_in: get("TOKENS_IN")?.parse().unwrap_or(0),
            tokens_out: get("TOKENS_OUT")?.parse().unwrap_or(0),
            termination_reason: {
                let v = get("TERMINATION_REASON")?;
                if v.is_empty() {
                    None
                } else {
                    Some(
                        TerminationReason::from_str(&v)
                            .map_err(|e| format!("bad TERMINATION_REASON: {e}"))?,
                    )
                }
            },
            kill_requested: get("KILL_REQUESTED")? == "true",
            tracker_cleanup_requested: get("TRACKER_CLEANUP_REQUESTED")? == "true",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_unescape_roundtrip() {
        let original = "hello\\world\nnewline";
        let escaped = escape_value(original);
        let unescaped = unescape_value(&escaped);
        assert_eq!(unescaped, original);
    }

    #[test]
    fn test_metadata_serialize_deserialize_roundtrip() {
        let meta = SessionMetadata {
            session_id: "proj-42-1".to_string(),
            status: SessionStatus::Working,
            created_at: "2026-03-08T00:00:00Z".to_string(),
            updated_at: "2026-03-08T00:01:00Z".to_string(),
            workspace_path: PathBuf::from("/tmp/ws"),
            agent: "claude-code".to_string(),
            runtime: "tmux".to_string(),
            issue_id: "42".to_string(),
            attempt: 1,
            branch: "feat/test".to_string(),
            base_branch: "main".to_string(),
            pr_url: String::new(),
            tokens_in: 100,
            tokens_out: 200,
            termination_reason: Some(TerminationReason::ManualKill),
            kill_requested: false,
            tracker_cleanup_requested: false,
        };
        let serialized = meta.serialize();
        let deserialized = SessionMetadata::deserialize(&serialized).unwrap();
        assert_eq!(deserialized.session_id, "proj-42-1");
        assert_eq!(deserialized.status, SessionStatus::Working);
        assert_eq!(
            deserialized.termination_reason,
            Some(TerminationReason::ManualKill)
        );
        assert_eq!(deserialized.tokens_in, 100);
        assert_eq!(deserialized.tokens_out, 200);
    }
}
