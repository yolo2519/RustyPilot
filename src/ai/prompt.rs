//! Prompt building utilities for AI requests.
//!
//! This module constructs structured prompts that include user queries
//! along with relevant context information (working directory, environment,
//! command history) to provide better AI command suggestions.

use crate::context::ContextSnapshot;

/// Build a comprehensive prompt for the AI including system context
pub fn build_prompt(user_query: &str, ctx: &ContextSnapshot) -> String {
    let mut prompt = String::new();

    // User's request
    prompt.push_str("USER REQUEST:\n");
    prompt.push_str(user_query);
    prompt.push_str("\n\n");

    // Current working directory
    prompt.push_str("CURRENT DIRECTORY:\n");
    prompt.push_str(&ctx.cwd);
    prompt.push_str("\n\n");

    // Recent command history (if available)
    if !ctx.recent_history.is_empty() {
        prompt.push_str("RECENT COMMAND HISTORY:\n");
        for (idx, cmd) in ctx.recent_history.iter().rev().take(5).enumerate() {
            prompt.push_str(&format!("  {}. {}\n", idx + 1, cmd));
        }
        prompt.push_str("\n");
    }

    // Recent terminal output
    if !ctx.recent_output.is_empty() {
        prompt.push_str("RECENT TERMINAL OUTPUT (last few lines):\n");
        for line in ctx.recent_output.iter().rev().take(6).rev() {
            prompt.push_str(&format!("  {}\n", line));
        }
        prompt.push_str("\n");
    }

    // Relevant environment variables
    if !ctx.env_vars.is_empty() {
        prompt.push_str("RELEVANT ENVIRONMENT:\n");
        
        // Filter to show only commonly useful env vars
        let relevant_vars = ["PATH", "HOME", "USER", "SHELL", "EDITOR", "LANG"];
        for (key, value) in &ctx.env_vars {
            if relevant_vars.contains(&key.as_str()) {
                // Truncate long values (like PATH)
                let display_value = if value.len() > 100 {
                    format!("{}...", &value[..100])
                } else {
                    value.clone()
                };
                prompt.push_str(&format!("  {}={}\n", key, display_value));
            }
        }
        prompt.push_str("\n");
    }

    // Instructions
    prompt.push_str("Please suggest a shell command to accomplish this task. ");
    prompt.push_str("Format your response with:\n");
    prompt.push_str("COMMAND: <the command>\n");
    prompt.push_str("EXPLANATION: <what it does>\n");
    prompt.push_str("ALTERNATIVES: <optional alternatives>\n");

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prompt_basic() {
        let ctx = ContextSnapshot {
            cwd: "/home/user/projects".to_string(),
            env_vars: vec![
                ("HOME".to_string(), "/home/user".to_string()),
                ("SHELL".to_string(), "/bin/bash".to_string()),
            ],
            recent_history: vec!["ls -la".to_string(), "cd projects".to_string()],
            recent_output: vec!["output line".to_string()],
        };

        let prompt = build_prompt("list all files", &ctx);

        assert!(prompt.contains("USER REQUEST:"));
        assert!(prompt.contains("list all files"));
        assert!(prompt.contains("CURRENT DIRECTORY:"));
        assert!(prompt.contains("/home/user/projects"));
        assert!(prompt.contains("RECENT COMMAND HISTORY:"));
        assert!(prompt.contains("ls -la"));
    }

    #[test]
    fn test_build_prompt_empty_context() {
        let ctx = ContextSnapshot {
            cwd: "/".to_string(),
            env_vars: vec![],
            recent_history: vec![],
            recent_output: vec![],
        };

        let prompt = build_prompt("help me", &ctx);

        assert!(prompt.contains("USER REQUEST:"));
        assert!(prompt.contains("help me"));
        assert!(prompt.contains("CURRENT DIRECTORY:"));
    }
}
