//! Prompt building utilities for AI requests.
//!
//! This module constructs structured prompts that include user queries
//! along with relevant context information (working directory, environment,
//! command history) to provide better AI command suggestions.
//!
//! User messages are formatted as JSON to enable reliable extraction of the
//! original user request from conversation history.

use crate::context::ContextSnapshot;
use serde::{Deserialize, Serialize};

/// System prompt that defines the AI assistant's behavior and personality.
pub const SYSTEM_PROMPT: &str = r#"You are an expert shell command assistant integrated into a terminal emulator. Your role is to help users execute shell commands safely and efficiently.

**Input Format**: User messages are JSON with "user_request" (what to respond to) and "context" (shell environment).
**Output Format**: Always respond in plain text (NOT JSON). Use the suggest_command tool for commands.

## CRITICAL RULE: Tool Usage for Commands

**You MUST use the `suggest_command` tool whenever you want to suggest ANY shell command.**
**NEVER write commands as plain text in your response. This is a hard requirement.**

When you think "the user should run X" or "try this command: X", you MUST call suggest_command instead.

Examples that REQUIRE tool use:
- "how do I list files" → MUST use suggest_command
- "delete all .tmp files" → MUST use suggest_command
- "help me find large files" → MUST use suggest_command
- "what's the command to..." → MUST use suggest_command
- User describes any task involving shell commands → MUST use suggest_command

The ONLY time you respond with plain text (no tool) is when:
- Answering general questions that don't involve running commands
- Asking for clarification before you can suggest a command
- Explaining concepts without suggesting a specific command to execute

## Guidelines

1. Focus on the "user_request" field - this is the user's actual question or task.
2. Use the "context" to understand the user's environment and provide relevant suggestions.
3. Explain what the command does and any potential side effects in the explanation field.
4. Use the risk_level field: low (safe/read-only), medium (modifies files), high (destructive/system-changing).
5. Consider the user's current directory and environment when suggesting commands.
6. Prefer portable POSIX-compliant commands when possible.

Be concise but thorough. Safety first."#;

/// Structured user prompt for JSON serialization.
///
/// This structure ensures that user requests can be reliably extracted from
/// conversation history, regardless of what the user types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPrompt {
    /// The user's original request (what they actually typed)
    pub user_request: String,
    /// Shell context information
    pub context: ContextSnapshot,
}

/// Build a complete prompt for the AI including context and user query.
///
/// Takes ownership of the context to avoid unnecessary cloning.
/// Note: Command output truncation and ANSI stripping are done earlier
/// (in snapshot_with_commands and append_output respectively).
///
/// # Errors
///
/// Returns an error if JSON serialization fails (should be extremely rare).
pub fn build_prompt(user_query: &str, ctx: ContextSnapshot) -> Result<String, serde_json::Error> {
    let prompt = UserPrompt {
        user_request: user_query.to_string(),
        context: ctx,
    };

    serde_json::to_string_pretty(&prompt)
}

/// Extract the original user request from a JSON-formatted prompt.
///
/// This is the inverse of `build_prompt()` - it extracts just the user's
/// original input without the context information.
pub fn extract_user_request(prompt_json: &str) -> Option<String> {
    serde_json::from_str::<UserPrompt>(prompt_json)
        .ok()
        .map(|p| p.user_request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::CommandRecord;

    #[test]
    fn test_build_prompt_basic() -> Result<(), Box<dyn std::error::Error>> {
        let ctx = ContextSnapshot {
            cwd: "/home/user/projects".to_string(),
            env_vars: vec![
                ("HOME".to_string(), "/home/user".to_string()),
                ("SHELL".to_string(), "/bin/bash".to_string()),
            ],
            recent_history: vec!["ls -la".to_string(), "cd projects".to_string()],
            recent_output: vec!["output line".to_string()],
            recent_commands: vec![
                CommandRecord {
                    command_line: "ls -la".to_string(),
                    output: "total 8\ndrwxr-xr-x  3 user  staff  96 Dec 13 10:00 .\n".to_string(),
                },
            ],
        };

        let prompt = build_prompt("list all files", ctx)?;

        // Verify it's valid JSON
        let parsed: UserPrompt = serde_json::from_str(&prompt)?;
        assert_eq!(parsed.user_request, "list all files");
        assert_eq!(parsed.context.cwd, "/home/user/projects");
        assert!(parsed.context.recent_output.contains(&"output line".to_string()));
        assert_eq!(parsed.context.recent_commands.len(), 1);
        assert_eq!(parsed.context.recent_commands[0].command_line, "ls -la");
        Ok(())
    }

    #[test]
    fn test_build_prompt_empty_context() -> Result<(), Box<dyn std::error::Error>> {
        let ctx = ContextSnapshot {
            cwd: "/".to_string(),
            env_vars: vec![],
            recent_history: vec![],
            recent_output: vec![],
            recent_commands: vec![],
        };

        let prompt = build_prompt("help me", ctx)?;

        let parsed: UserPrompt = serde_json::from_str(&prompt)?;
        assert_eq!(parsed.user_request, "help me");
        assert_eq!(parsed.context.cwd, "/");
        assert_eq!(parsed.context.recent_commands.len(), 0);
        Ok(())
    }

    #[test]
    fn test_extract_user_request() -> Result<(), Box<dyn std::error::Error>> {
        let ctx = ContextSnapshot {
            cwd: "/tmp".to_string(),
            env_vars: vec![],
            recent_history: vec![],
            recent_output: vec![],
            recent_commands: vec![],
        };

        let prompt = build_prompt("find large files", ctx)?;
        let extracted = extract_user_request(&prompt);

        assert_eq!(extracted, Some("find large files".to_string()));
        Ok(())
    }

    #[test]
    fn test_extract_user_request_with_special_chars() -> Result<(), Box<dyn std::error::Error>> {
        let ctx = ContextSnapshot {
            cwd: "/".to_string(),
            env_vars: vec![],
            recent_history: vec![],
            recent_output: vec![],
            recent_commands: vec![],
        };

        // Test with special characters that need JSON escaping
        let query = "find files with \"quotes\" and\nnewlines";
        let prompt = build_prompt(query, ctx)?;
        let extracted = extract_user_request(&prompt);

        assert_eq!(extracted, Some(query.to_string()));
        Ok(())
    }
}
