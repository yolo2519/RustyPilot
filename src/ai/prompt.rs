//! Prompt building utilities for AI requests.
//!
//! This module constructs structured prompts that include user queries
//! along with relevant context information (working directory, environment,
//! command history) to provide better AI command suggestions.

use crate::context::ContextSnapshot;

pub fn build_prompt(user_query: &str, ctx: &ContextSnapshot) -> String {
    // TODO: stuff cwd / env / history etc. into prompt
    format!(
        "You are a shell command assistant.\n\
         Current directory: {cwd}\n\
         User query: {query}\n\
         Return a single safe shell command and explanation.",
        cwd = ctx.cwd,
        query = user_query
    )
}
