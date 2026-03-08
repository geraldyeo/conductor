use std::{collections::HashMap, path::Path, time::Duration};
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("command timed out")]
    Timeout,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
}

pub struct CommandRunner;

impl CommandRunner {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(
        &self,
        args: &[&str],
        env: Option<HashMap<String, String>>,
        timeout: Option<Duration>,
    ) -> Result<CommandOutput, CommandError> {
        let (program, rest) = args.split_first().expect("args must not be empty");
        let mut cmd = Command::new(program);
        cmd.args(rest);
        if let Some(env) = env {
            cmd.envs(env);
        }
        run_with_timeout(&mut cmd, timeout).await
    }

    pub async fn run_in_dir(
        &self,
        args: &[&str],
        cwd: &Path,
        env: Option<HashMap<String, String>>,
        timeout: Option<Duration>,
    ) -> Result<CommandOutput, CommandError> {
        let (program, rest) = args.split_first().expect("args must not be empty");
        let mut cmd = Command::new(program);
        cmd.args(rest).current_dir(cwd);
        if let Some(env) = env {
            cmd.envs(env);
        }
        run_with_timeout(&mut cmd, timeout).await
    }
}

impl Default for CommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

async fn run_with_timeout(
    cmd: &mut Command,
    timeout: Option<Duration>,
) -> Result<CommandOutput, CommandError> {
    let future = cmd.output();
    let output = match timeout {
        Some(t) => tokio::time::timeout(t, future)
            .await
            .map_err(|_| CommandError::Timeout)?,
        None => future.await,
    }?;
    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
        success: output.status.success(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_successful_command() {
        let runner = CommandRunner::new();
        let result = runner.run(&["echo", "hello"], None, None).await.unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_failed_command() {
        let runner = CommandRunner::new();
        let result = runner.run(&["false"], None, None).await.unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_command_with_env() {
        let runner = CommandRunner::new();
        let env = [("MY_VAR".to_string(), "world".to_string())].into();
        let result = runner
            .run(&["sh", "-c", "echo $MY_VAR"], Some(env), None)
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "world");
    }

    #[tokio::test]
    async fn test_command_timeout() {
        let runner = CommandRunner::new();
        let result = runner
            .run(&["sleep", "10"], None, Some(Duration::from_millis(100)))
            .await;
        assert!(matches!(result, Err(CommandError::Timeout)));
    }
}
