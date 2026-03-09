use strum::{Display, EnumString};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum SessionStatus {
    Spawning,
    Working,
    PrOpen,
    ReviewPending,
    Approved,
    Mergeable,
    CiFailed,
    ChangesRequested,
    NeedsInput,
    Stuck,
    Killed,
    Terminated,
    Done,
    Cleanup,
    Errored,
    Merged,
}

impl SessionStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SessionStatus::Killed
                | SessionStatus::Terminated
                | SessionStatus::Done
                | SessionStatus::Cleanup
                | SessionStatus::Errored
                | SessionStatus::Merged
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum TerminationReason {
    BudgetExceeded,
    ManualKill,
    StallTimeout,
    TrackerTerminal,
    AgentExit,
    SpawnFailed,
    MaxRetriesExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_session_status_roundtrip() {
        let s = SessionStatus::Working;
        let serialized = s.to_string();
        assert_eq!(serialized, "working");
        let parsed = SessionStatus::from_str(&serialized).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn test_all_16_statuses_parse() {
        let statuses = [
            "spawning",
            "working",
            "pr_open",
            "review_pending",
            "approved",
            "mergeable",
            "ci_failed",
            "changes_requested",
            "needs_input",
            "stuck",
            "killed",
            "terminated",
            "done",
            "cleanup",
            "errored",
            "merged",
        ];
        for s in &statuses {
            SessionStatus::from_str(s).unwrap_or_else(|_| panic!("failed to parse: {s}"));
        }
    }

    #[test]
    fn test_is_terminal() {
        assert!(SessionStatus::Killed.is_terminal());
        assert!(SessionStatus::Terminated.is_terminal());
        assert!(SessionStatus::Done.is_terminal());
        assert!(SessionStatus::Cleanup.is_terminal());
        assert!(SessionStatus::Errored.is_terminal());
        assert!(SessionStatus::Merged.is_terminal());
        assert!(!SessionStatus::Working.is_terminal());
        assert!(!SessionStatus::Spawning.is_terminal());
        assert!(!SessionStatus::PrOpen.is_terminal());
    }

    #[test]
    fn test_termination_reason_roundtrip() {
        let r = TerminationReason::ManualKill;
        let serialized = r.to_string();
        assert_eq!(serialized, "manual_kill");
        let parsed = TerminationReason::from_str(&serialized).unwrap();
        assert_eq!(parsed, r);
    }
}
