use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    General(String),
    #[error("config error: {0}")]
    Config(#[from] conductor_core::config::ConfigError),
    #[error("{0}")]
    Ipc(#[from] crate::ipc::client::IpcError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Resolve(#[from] crate::resolve::ResolveError),
}

/// Map CliError to process exit code.
pub fn exit_code(e: &CliError) -> i32 {
    match e {
        CliError::Config(_) => 3,
        CliError::Ipc(crate::ipc::client::IpcError::NotRunning(_)) => 4,
        CliError::General(_) | CliError::Io(_) | CliError::Ipc(_) | CliError::Resolve(_) => 1,
    }
}

pub fn print_error(e: &CliError, json: bool) {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({"error": {"code": "error", "message": e.to_string()}})
        );
    } else {
        eprintln!("error: {e}");
    }
}
