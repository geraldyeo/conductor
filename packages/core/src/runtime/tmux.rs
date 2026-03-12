use super::{Runtime, RuntimeError};
use crate::types::{PluginMeta, RuntimeStep};
use crate::utils::CommandRunner;
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};

pub struct TmuxRuntime {
    runner: Arc<CommandRunner>,
}

impl TmuxRuntime {
    pub fn new(runner: Arc<CommandRunner>) -> Self {
        Self { runner }
    }

    fn tmux_session_name(session_id: &str) -> String {
        // tmux session names: strip invalid chars, prefix with "ao-" if not already present.
        let safe: String = session_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        if safe.starts_with("ao-") {
            safe
        } else {
            format!("ao-{safe}")
        }
    }
}

#[async_trait]
impl Runtime for TmuxRuntime {
    fn meta(&self) -> PluginMeta {
        PluginMeta {
            name: "tmux",
            version: "0.1.0",
            description: "tmux terminal runtime",
        }
    }

    async fn execute_step(&self, session_id: &str, step: &RuntimeStep) -> Result<(), RuntimeError> {
        let name = Self::tmux_session_name(session_id);
        match step {
            RuntimeStep::Create {
                command,
                env,
                working_dir,
            } => {
                let wd = working_dir.to_string_lossy().to_string();
                let mut full_args: Vec<String> = vec![
                    "tmux".to_string(),
                    "new-session".to_string(),
                    "-d".to_string(),
                    "-s".to_string(),
                    name,
                    "-c".to_string(),
                    wd,
                ];
                // Build env args
                for (k, v) in env {
                    full_args.push("-e".to_string());
                    full_args.push(format!("{k}={v}"));
                }
                full_args.push("--".to_string());
                full_args.extend(command.iter().cloned());
                let args_ref: Vec<&str> = full_args.iter().map(|s| s.as_str()).collect();
                let out = self
                    .runner
                    .run(&args_ref, None, None)
                    .await
                    .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
                if !out.success {
                    return Err(RuntimeError::CommandFailed(out.stderr));
                }
                Ok(())
            }
            RuntimeStep::WaitForReady { timeout } => {
                let deadline = tokio::time::Instant::now() + *timeout;
                while tokio::time::Instant::now() < deadline {
                    let out = self
                        .runner
                        .run(&["tmux", "has-session", "-t", &name], None, None)
                        .await
                        .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
                    if out.success {
                        return Ok(());
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                Err(RuntimeError::CommandFailed("WaitForReady timed out".into()))
            }
            RuntimeStep::SendMessage { content } => {
                let out = self
                    .runner
                    .run(
                        &["tmux", "send-keys", "-t", &name, "--", content, "Enter"],
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
                if !out.success {
                    return Err(RuntimeError::CommandFailed(out.stderr));
                }
                Ok(())
            }
            RuntimeStep::SendBuffer { content } => {
                // Write to tmux buffer then paste
                let out = self
                    .runner
                    .run(&["tmux", "set-buffer", "--", content], None, None)
                    .await
                    .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
                if !out.success {
                    return Err(RuntimeError::CommandFailed(out.stderr));
                }
                let out = self
                    .runner
                    .run(&["tmux", "paste-buffer", "-t", &name], None, None)
                    .await
                    .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
                if !out.success {
                    return Err(RuntimeError::CommandFailed(out.stderr));
                }
                Ok(())
            }
            RuntimeStep::SendProtocol { .. } => Err(RuntimeError::UnsupportedStep(
                "SendProtocol not supported by tmux".into(),
            )),
        }
    }

    async fn get_output(&self, session_id: &str, lines: usize) -> Result<String, RuntimeError> {
        let name = Self::tmux_session_name(session_id);
        let start_line = format!("-{lines}");
        let out = self
            .runner
            .run(
                &["tmux", "capture-pane", "-p", "-t", &name, "-S", &start_line],
                None,
                None,
            )
            .await
            .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
        Ok(out.stdout)
    }

    async fn is_alive(&self, session_id: &str) -> Result<bool, RuntimeError> {
        let name = Self::tmux_session_name(session_id);
        let out = self
            .runner
            .run(&["tmux", "has-session", "-t", &name], None, None)
            .await
            .map_err(|e| RuntimeError::CommandFailed(e.to_string()))?;
        Ok(out.success)
    }

    async fn destroy(&self, session_id: &str) -> Result<(), RuntimeError> {
        let name = Self::tmux_session_name(session_id);
        let _ = self
            .runner
            .run(&["tmux", "kill-session", "-t", &name], None, None)
            .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RuntimeStep;
    use std::{collections::HashMap, path::PathBuf};

    fn make_runtime() -> TmuxRuntime {
        TmuxRuntime::new(Arc::new(CommandRunner::new()))
    }

    #[test]
    fn test_tmux_session_name_sanitized() {
        assert_eq!(TmuxRuntime::tmux_session_name("proj-42-1"), "ao-proj-42-1");
        assert_eq!(TmuxRuntime::tmux_session_name("proj/42:1"), "ao-proj-42-1");
    }

    #[test]
    fn test_tmux_session_name_no_double_prefix_for_ao_ids() {
        // Session IDs already start with "ao-"; tmux name must not double-prefix.
        assert_eq!(
            TmuxRuntime::tmux_session_name("ao-243b720719617b18"),
            "ao-243b720719617b18"
        );
    }

    #[tokio::test]
    #[ignore] // Requires tmux to be installed
    async fn test_create_and_destroy_session() {
        let runtime = make_runtime();
        let session_id = "ao-test-session-999";
        // Cleanup first in case prior test left it
        let _ = runtime.destroy(session_id).await;

        let step = RuntimeStep::Create {
            command: vec!["sleep".to_string(), "60".to_string()],
            env: HashMap::new(),
            working_dir: PathBuf::from("/tmp"),
        };
        runtime.execute_step(session_id, &step).await.unwrap();
        assert!(runtime.is_alive(session_id).await.unwrap());
        runtime.destroy(session_id).await.unwrap();
        assert!(!runtime.is_alive(session_id).await.unwrap());
    }
}
