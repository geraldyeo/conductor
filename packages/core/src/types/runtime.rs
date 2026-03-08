use std::{collections::HashMap, path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub enum RuntimeStep {
    Create {
        command: Vec<String>,
        env: HashMap<String, String>,
        working_dir: PathBuf,
    },
    WaitForReady {
        timeout: Duration,
    },
    SendMessage {
        content: String,
    },
    SendBuffer {
        content: String,
    },
    SendProtocol {
        payload: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub steps: Vec<RuntimeStep>,
}
