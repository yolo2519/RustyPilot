//! Prompt building utilities for AI requests.
//!
//! This module constructs structured prompts that include user queries
//! along with relevant context information (working directory, environment,
//! command history) to provide better AI command suggestions.

use crate::context::ContextSnapshot;

/// System prompt that defines the AI assistant's behavior.
const SYSTEM_PROMPT: &str = r#"You are an expert shell command assistant integrated into a terminal emulator. Your role is to help users execute shell commands safely and efficiently.

Guidelines:
1. Provide a single, safe shell command that accomplishes the user's goal
2. Explain what the command does and any potential side effects
3. If the command could be dangerous (deleting files, modifying system settings), warn the user
4. Consider the user's current directory and environment when suggesting commands
5. Prefer portable POSIX-compliant commands when possible
6. If you're unsure about the user's intent, ask for clarification

Response format:
- First, provide a brief explanation of what you'll do
- Then provide the command
- Finally, note any warnings or alternatives if relevant"#;

/// Build a complete prompt for the AI including context and user query.
pub fn build_prompt(user_query: &str, ctx: &ContextSnapshot) -> String {
    format!(
        "{system}\n\n\
         --- Context ---\n\
         {context}\n\
         --- User Request ---\n\
         {query}\n\n\
         Please provide a shell command to help with this request.",
        system = SYSTEM_PROMPT,
        context = ctx.format_for_prompt(),
        query = user_query
    )
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
