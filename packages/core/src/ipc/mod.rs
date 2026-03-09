use std::path::PathBuf;

/// Returns the path to the orchestrator Unix domain socket.
/// This is a stub for MVP; the post-MVP daemon will use this for IPC.
pub fn socket_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".agent-orchestrator")
        .join("orchestrator.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_path_is_absolute() {
        let path = socket_path();
        assert!(path.is_absolute(), "socket path should be absolute: {}", path.display());
    }

    #[test]
    fn test_socket_path_under_agent_orchestrator() {
        let path = socket_path();
        assert!(
            path.to_string_lossy().contains(".agent-orchestrator"),
            "socket path should be under .agent-orchestrator: {}",
            path.display()
        );
    }

    #[test]
    fn test_socket_path_filename() {
        let path = socket_path();
        assert_eq!(
            path.file_name().and_then(|f| f.to_str()),
            Some("orchestrator.sock"),
        );
    }
}
