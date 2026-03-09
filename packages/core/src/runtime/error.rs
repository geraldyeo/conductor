use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("unsupported step: {0}")]
    UnsupportedStep(String),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
