use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config not found (searched CWD ancestors and home directory)")]
    NotFound,
    #[error("AO_CONFIG_PATH={0:?} does not exist")]
    EnvPathNotFound(PathBuf),
    #[error("could not determine home directory")]
    NoHomeDir,
    #[error("{path}: parse error: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yml::Error,
    },
    #[error("{path}: validation failed:\n{violations}")]
    Validation { path: PathBuf, violations: String },
    #[error("{path}: {message}")]
    Io { path: PathBuf, message: String },
}
