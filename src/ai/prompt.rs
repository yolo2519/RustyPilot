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
    pub context: PromptContext,
}

/// Context information included in the prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptContext {
    /// Current working directory
    pub cwd: String,
    /// Relevant environment variables
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub env: Vec<(String, String)>,
    /// Recent command history
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub recent_history: Vec<String>,
    /// Recent terminal output
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub recent_output: Vec<String>,
    /// Recent commands with their outputs
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub recent_commands: Vec<CommandWithOutput>,
}

/// A command with its output for context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandWithOutput {
    pub command: String,
    pub output: String,
}

/// Build a complete prompt for the AI including context and user query.
///
/// Returns a JSON-formatted string that can be reliably parsed to extract
/// the original user request.
///
/// # Errors
///
/// Returns an error if JSON serialization fails (should be extremely rare).
pub fn build_prompt(user_query: &str, ctx: &ContextSnapshot) -> Result<String, serde_json::Error> {
    // Convert command records to the format expected by PromptContext
    let recent_commands: Vec<CommandWithOutput> = ctx
        .recent_commands
        .iter()
        .map(|record| CommandWithOutput {
            command: record.command_line.clone(),
            output: truncate_text(&record.output, 2048),
        })
        .collect();

    let prompt = UserPrompt {
        user_request: user_query.to_string(),
        context: PromptContext {
            cwd: ctx.cwd.clone(),
            env: ctx.env_vars.clone(),
            recent_history: ctx.recent_history.clone(),
            recent_output: ctx.recent_output.iter().rev().take(6).rev().cloned().collect(),
            recent_commands,
        },
    };

    serde_json::to_string_pretty(&prompt)
}

/// Truncate text to a maximum number of bytes, preserving UTF-8 boundaries.
fn truncate_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    
    // Find the last valid UTF-8 character boundary within max_bytes
    if let Some((idx, _)) = text
        .char_indices()
        .take_while(|(i, _)| *i < max_bytes)
        .last()
    {
        let mut result = text[..=idx].to_string();
        if idx < text.len() - 1 {
            result.push_str("...[truncated]");
        }
        result
    } else {
        String::new()
    }
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
    fn test_build_prompt_basic() -> Result<(), Box<dyn std::error::Error>> {
        use crate::context::CommandRecord;
        
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

        let prompt = build_prompt("list all files", &ctx)?;

        // Verify it's valid JSON
        let parsed: UserPrompt = serde_json::from_str(&prompt)?;
        assert_eq!(parsed.user_request, "list all files");
        assert_eq!(parsed.context.cwd, "/home/user/projects");
        assert!(parsed.context.recent_output.contains(&"output line".to_string()));
        assert_eq!(parsed.context.recent_commands.len(), 1);
        assert_eq!(parsed.context.recent_commands[0].command, "ls -la");
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

        let prompt = build_prompt("help me", &ctx)?;

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

        let prompt = build_prompt("find large files", &ctx)?;
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
        let prompt = build_prompt(query, &ctx)?;
        let extracted = extract_user_request(&prompt);

        assert_eq!(extracted, Some(query.to_string()));
        Ok(())
    }
}
