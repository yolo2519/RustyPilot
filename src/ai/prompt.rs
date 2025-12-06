//! Prompt building utilities for AI requests.
//!
//! This module constructs structured prompts that include user queries
//! along with relevant context information (working directory, environment,
//! command history) to provide better AI command suggestions.

use crate::context::ContextSnapshot;

/// System prompt that defines the AI assistant's behavior and personality.
pub const SYSTEM_PROMPT: &str = r#"You are an expert shell command assistant integrated into a terminal emulator. Your role is to help users execute shell commands safely and efficiently.

Guidelines:
1. Understand the user's natural language request.
2. Suggest a single, safe, correct shell command that accomplishes the goal.
3. Explain what the command does and any potential side effects.
4. If the command could be dangerous (deleting files, modifying system settings), warn the user.
5. Consider the user's current directory and environment.
6. Prefer portable POSIX-compliant commands when possible.

Response Format:
You must strictly follow this format for the parser to understand your response:

COMMAND: <the actual command>
EXPLANATION: <what it does and why>
ALTERNATIVES: <optional alternative commands, one per line>

Be concise but thorough. Safety first."#;

/// Build a complete prompt for the AI including context and user query.
pub fn build_prompt(user_query: &str, ctx: &ContextSnapshot) -> String {
    let mut prompt = String::new();

    // User's request
    prompt.push_str("USER REQUEST:\n");
    prompt.push_str(user_query);
    prompt.push_str("\n\n");

    // System Context
    prompt.push_str("--- Context ---\n");
    prompt.push_str(&ctx.format_for_prompt());

    // Recent terminal output (not included in format_for_prompt)
    if !ctx.recent_output.is_empty() {
        prompt.push_str("\nRECENT TERMINAL OUTPUT (last few lines):\n");
        for line in ctx.recent_output.iter().rev().take(6).rev() {
            prompt.push_str(&format!("  {}\n", line));
        }
    }
    prompt.push_str("\n");

    // Instructions for response format (reinforced here for reliability)
    prompt.push_str("Please suggest a shell command to accomplish this task.\n");
    prompt.push_str("Remember to use the strict format:\n");
    prompt.push_str("COMMAND: <command>\n");
    prompt.push_str("EXPLANATION: <explanation>\n");
    prompt.push_str("ALTERNATIVES: <alternatives>\n");

    prompt
}

/// Build a prompt for command explanation.
pub fn build_explain_prompt(command: &str, ctx: &ContextSnapshot) -> String {
    format!(
        "You are a shell command expert. Explain what this command does in detail:\n\n\
         Command: {command}\n\n\
         Context:\n{context}\n\n\
         Provide a clear explanation including:\n\
         1. What each part of the command does\n\
         2. What files or resources it will affect\n\
         3. Any potential risks or side effects\n\
         4. Safer alternatives if applicable",
        command = command,
        context = ctx.format_for_prompt()
    )
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
        assert!(prompt.contains("Current directory: /home/user/projects"));
        assert!(prompt.contains("RECENT TERMINAL OUTPUT"));
        assert!(prompt.contains("output line"));
        assert!(prompt.contains("COMMAND:"));
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
        assert!(prompt.contains("Current directory: /"));
    }
}
