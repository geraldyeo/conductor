use super::{Agent, GatherContext, SessionInfo};
use crate::types::{LaunchPlan, RuntimeStep};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub struct ClaudeCodeAgent {
    session_prefix: String,
}

impl ClaudeCodeAgent {
    pub fn new(session_prefix: String) -> Self {
        Self { session_prefix }
    }
}

fn escape_prompt(s: &str) -> String {
    s.replace('\'', "'\\''")
}

impl Agent for ClaudeCodeAgent {
    fn launch_plan(&self, session_id: &str, prompt: &str) -> LaunchPlan {
        let tmux_name = format!("{}-{}", self.session_prefix, session_id);
        LaunchPlan {
            steps: vec![
                RuntimeStep::Create {
                    command: vec![
                        "tmux".to_string(),
                        "new-session".to_string(),
                        "-d".to_string(),
                        "-s".to_string(),
                        tmux_name,
                        "-x".to_string(),
                        "220".to_string(),
                        "-y".to_string(),
                        "50".to_string(),
                    ],
                    env: HashMap::new(),
                    working_dir: PathBuf::from("."),
                },
                RuntimeStep::WaitForReady {
                    timeout: Duration::from_secs(2),
                },
                RuntimeStep::SendBuffer {
                    content: format!(
                        "claude --dangerously-skip-permissions '{}'\n",
                        escape_prompt(prompt)
                    ),
                },
            ],
        }
    }

    fn detect_activity(&self, ctx: &GatherContext) -> bool {
        let output = &ctx.raw_output;
        output.contains("Thinking...")
            || output.contains("Reading")
            || output.contains("Writing")
            || output.contains("Running")
            || output.contains("> ")
    }

    fn parse_session_info(&self, ctx: &GatherContext) -> SessionInfo {
        let mut info = SessionInfo::default();

        for line in ctx.raw_output.lines() {
            if let Some(cost_start) = line.find("Cost: $") {
                let after_dollar = &line[cost_start + 7..];
                if let Some(end) = after_dollar.find(|c: char| !c.is_ascii_digit() && c != '.') {
                    if let Ok(cost) = after_dollar[..end].parse::<f64>() {
                        info.cost_usd = Some(cost);
                    }
                } else if let Ok(cost) = after_dollar.parse::<f64>() {
                    info.cost_usd = Some(cost);
                }

                // Try to parse input tokens
                if let Some(input_pos) = line.find("input") {
                    info.input_tokens = parse_token_count(line, input_pos);
                }
                // Try to parse output tokens
                if let Some(output_pos) = line.find("output") {
                    info.output_tokens = parse_token_count(line, output_pos);
                }
            }
        }

        info
    }
}

fn parse_token_count(line: &str, keyword_pos: usize) -> Option<u64> {
    // Look backwards from keyword_pos for a number like "123k" or "1.5k" or "1234"
    let before = &line[..keyword_pos].trim_end();
    let last_word = before.rsplit_once(|c: char| c.is_whitespace() || c == '(');
    let token_str = last_word.map(|(_, w)| w).unwrap_or(before);
    let token_str = token_str.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != 'k');

    if let Some(num_str) = token_str.strip_suffix('k') {
        num_str
            .parse::<f64>()
            .ok()
            .map(|n| (n * 1000.0) as u64)
    } else {
        token_str.parse::<u64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RuntimeStep;

    fn make_agent() -> ClaudeCodeAgent {
        ClaudeCodeAgent::new("ao".to_string())
    }

    #[test]
    fn test_launch_plan_has_three_steps() {
        let agent = make_agent();
        let plan = agent.launch_plan("my-session", "fix the bug");
        assert_eq!(plan.steps.len(), 3);
    }

    #[test]
    fn test_launch_plan_first_step_is_create() {
        let agent = make_agent();
        let plan = agent.launch_plan("my-session", "fix the bug");
        if let RuntimeStep::Create { command, .. } = &plan.steps[0] {
            assert_eq!(command[0], "tmux");
        } else {
            panic!("first step should be Create");
        }
    }

    #[test]
    fn test_detect_activity_thinking() {
        let agent = make_agent();
        let ctx = GatherContext {
            session_id: "s".to_string(),
            raw_output: "Some output\nThinking...\nMore output".to_string(),
            last_activity_at: 0,
        };
        assert!(agent.detect_activity(&ctx));
    }

    #[test]
    fn test_detect_activity_idle() {
        let agent = make_agent();
        let ctx = GatherContext {
            session_id: "s".to_string(),
            raw_output: "$ ".to_string(),
            last_activity_at: 0,
        };
        assert!(!agent.detect_activity(&ctx));
    }

    #[test]
    fn test_prompt_escaping() {
        let agent = make_agent();
        let plan = agent.launch_plan("s", "say 'hello'");
        if let RuntimeStep::SendBuffer { content } = &plan.steps[2] {
            assert!(
                content.contains("say '\\''hello'\\''"),
                "expected escaped quotes, got: {content}"
            );
        } else {
            panic!("third step should be SendBuffer");
        }
    }
}
