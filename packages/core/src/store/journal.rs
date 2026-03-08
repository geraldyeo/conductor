use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub action: String,
    pub target: String,
    pub timestamp: String,
    pub dedupe_key: String,
    pub result: JournalResult,
    pub error_code: Option<String>,
    pub attempt: u32,
    pub actor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JournalResult {
    Success,
    Failed,
    Skipped,
}
