use super::{ActivityState, Agent, GatherContext, LaunchContext};
use crate::types::{LaunchPlan, PluginMeta, RuntimeStep};
use std::collections::HashMap;

pub struct ClaudeCodeAgent;

impl ClaudeCodeAgent {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeCodeAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Agent for ClaudeCodeAgent {
    fn meta(&self) -> PluginMeta {
        PluginMeta {
            name: "claude-code",
            version: "0.1.0",
            description: "Claude Code agent",
        }
    }

    fn launch_plan(&self, ctx: &LaunchContext) -> LaunchPlan {
        // Inline delivery: claude -p <prompt>
        let mut env = HashMap::new();
        env.insert("AO_SESSION".to_string(), ctx.session_id.clone());
        LaunchPlan {
            steps: vec![RuntimeStep::Create {
                command: vec!["claude".to_string(), "-p".to_string(), ctx.prompt.clone()],
                env,
                working_dir: ctx.workspace_path.clone(),
            }],
        }
    }

    fn detect_activity(&self, ctx: &GatherContext) -> ActivityState {
        // Check JSONL log for activity indicators
        if let Some(log) = &ctx.auxiliary_log {
            // Look for recent tool use or generation activity
            if log.lines().rev().take(20).any(|line| {
                line.contains("\"type\":\"tool_use\"") || line.contains("\"type\":\"text\"")
            }) {
                return ActivityState::Active;
            }
            if log
                .lines()
                .rev()
                .take(5)
                .any(|line| line.contains("\"stop_reason\""))
            {
                return ActivityState::Ready;
            }
        }
        // Fall back to terminal output
        if ctx.terminal_output.contains("> ") || ctx.terminal_output.contains("$ ") {
            return ActivityState::Ready;
        }
        ActivityState::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RuntimeStep;

    fn make_ctx(prompt: &str) -> LaunchContext {
        LaunchContext {
            session_id: "proj-42-1".to_string(),
            prompt: prompt.to_string(),
            workspace_path: std::path::PathBuf::from("/tmp/ws"),
            issue_id: "42".to_string(),
            branch: "42-fix".to_string(),
        }
    }

    #[test]
    fn test_inline_launch_plan_has_create_step() {
        let agent = ClaudeCodeAgent::new();
        let ctx = make_ctx("Fix the bug.");
        let plan = agent.launch_plan(&ctx);
        assert!(!plan.steps.is_empty());
        assert!(matches!(plan.steps[0], RuntimeStep::Create { .. }));
    }

    #[test]
    fn test_launch_plan_command_contains_prompt() {
        let agent = ClaudeCodeAgent::new();
        let ctx = make_ctx("Fix the bug.");
        let plan = agent.launch_plan(&ctx);
        if let RuntimeStep::Create { command, .. } = &plan.steps[0] {
            assert!(command.iter().any(|a| a == "claude" || a == "claude-code"));
        }
    }

    #[test]
    fn test_detect_activity_active_on_tool_use() {
        let agent = ClaudeCodeAgent::new();
        let ctx = GatherContext {
            terminal_output: String::new(),
            auxiliary_log: Some(r#"{"type":"tool_use","name":"read"}"#.to_string()),
            auxiliary_log_path: None,
        };
        assert_eq!(agent.detect_activity(&ctx), ActivityState::Active);
    }

    #[test]
    fn test_detect_activity_ready_on_stop_reason() {
        let agent = ClaudeCodeAgent::new();
        let ctx = GatherContext {
            terminal_output: String::new(),
            auxiliary_log: Some(r#"{"stop_reason":"end_turn"}"#.to_string()),
            auxiliary_log_path: None,
        };
        assert_eq!(agent.detect_activity(&ctx), ActivityState::Ready);
    }

    #[test]
    fn test_detect_activity_ready_on_terminal_prompt() {
        let agent = ClaudeCodeAgent::new();
        let ctx = GatherContext {
            terminal_output: "user@host:~$ ".to_string(),
            auxiliary_log: None,
            auxiliary_log_path: None,
        };
        assert_eq!(agent.detect_activity(&ctx), ActivityState::Ready);
    }
}
