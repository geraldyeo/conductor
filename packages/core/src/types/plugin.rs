use thiserror::Error;

#[derive(Debug, Clone)]
pub struct PluginMeta {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin not implemented: {0}")]
    NotImplemented(String),
    #[error("unknown plugin: {0}")]
    UnknownPlugin(String),
    #[error("plugin validation failed: {0}")]
    ValidationFailed(String),
}
