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
        CliError::Resolve(crate::resolve::ResolveError::NotFound(_)) => 3,
        CliError::General(_) | CliError::Io(_) | CliError::Ipc(_) | CliError::Resolve(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::ResolveError;

    #[test]
    fn test_exit_code_resolve_not_found_is_3() {
        let e = CliError::Resolve(ResolveError::NotFound("ghost".to_string()));
        assert_eq!(exit_code(&e), 3);
    }

    #[test]
    fn test_exit_code_resolve_ambiguous_is_1() {
        let e = CliError::Resolve(ResolveError::Ambiguous("a, b".to_string()));
        assert_eq!(exit_code(&e), 1);
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
