//! Context management for capturing shell environment state.
//!
//! This module collects and provides contextual information about the current
//! shell session including working directory, environment variables, and command
//! history to enhance AI command suggestions.

mod command_log;
mod cwd;
mod env;
mod history;

pub use command_log::{CommandLog, CommandRecord};
pub use cwd::CurrentDir;
pub use env::Environment;
pub use history::History;

/// Manages all context information for AI suggestions.
#[derive(Debug)]
pub struct ContextManager {
    pub env: Environment,
    pub cwd: CurrentDir,
    pub history: History,
    recent_output: std::collections::VecDeque<String>,
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            env: Environment::capture(),
            cwd: CurrentDir::capture().unwrap_or_default(),
            history: History::new(),
            recent_output: std::collections::VecDeque::new(),
        }
    }

    /// Create a snapshot of the current context for AI consumption.
    pub fn snapshot(&self) -> ContextSnapshot {
        ContextSnapshot {
            cwd: self.cwd.path.clone(),
            env_vars: self.env.filtered_vars(),
            recent_history: self.history.recent(20),
            recent_output: self.recent_output.iter().cloned().collect(),
            recent_commands: Vec::new(), // Filled by caller with ShellManager data
        }
    }

    /// Create a snapshot with command records from ShellManager.
    pub fn snapshot_with_commands(&self, command_records: Vec<CommandRecord>) -> ContextSnapshot {
        ContextSnapshot {
            cwd: self.cwd.path.clone(),
            env_vars: self.env.filtered_vars(),
            recent_history: self.history.recent(20),
            recent_output: self.recent_output.iter().cloned().collect(),
            recent_commands: command_records,
        }
    }

    pub fn push_output(&mut self, chunk: String) {
        if chunk.trim().is_empty() {
            return;
        }
        const MAX_OUTPUT_LINES: usize = 20;
        for line in chunk.lines() {
            if line.trim().is_empty() {
                continue;
            }
            self.recent_output.push_back(line.trim().to_string());
            while self.recent_output.len() > MAX_OUTPUT_LINES {
                self.recent_output.pop_front();
            }
        }
    }

    /// Update the current working directory.
    pub fn update_cwd(&mut self, new_path: String) {
        self.cwd.update(new_path);
    }

    /// Update CWD from an OSC 7 sequence.
    pub fn update_cwd_from_osc7(&mut self, osc_payload: &str) -> bool {
        self.cwd.update_from_osc7(osc_payload)
    }

    /// Add a command to history.
    pub fn add_to_history(&mut self, command: String) {
        self.history.push(command);
    }

    /// Refresh environment variables from the current process.
    pub fn refresh_env(&mut self) {
        self.env = Environment::capture();
    }
}

/// A snapshot of context information for AI prompt building.
#[derive(Clone, Debug)]
pub struct ContextSnapshot {
    pub cwd: String,
    pub env_vars: Vec<(String, String)>,
    pub recent_history: Vec<String>,
    pub recent_output: Vec<String>,
    /// Recent commands with their outputs (command_line, output)
    pub recent_commands: Vec<CommandRecord>,
}

impl ContextSnapshot {
    /// Format context as a string for AI prompts.
    pub fn format_for_prompt(&self) -> String {
        let mut result = String::new();
        
        // Current directory
        result.push_str(&format!("Current directory: {}\n", self.cwd));
        
        // Recent commands with outputs (if any)
        if !self.recent_commands.is_empty() {
            result.push_str("\nRecent commands and outputs:\n");
            for record in &self.recent_commands {
                result.push_str(&format!("\n$ {}\n", record.command_line));
                // Truncate output to reasonable size (max ~2KB per command)
                let output_preview = truncate_output(&record.output, 2048);
                if !output_preview.is_empty() {
                    result.push_str(&output_preview);
                    result.push('\n');
                }
            }
        } else if !self.recent_history.is_empty() {
            // Fallback to history if no command records
            result.push_str("\nRecent commands:\n");
            for (i, cmd) in self.recent_history.iter().rev().take(5).enumerate() {
                result.push_str(&format!("  {}. {}\n", i + 1, cmd));
            }
        }
        
        // Key environment variables
        if !self.env_vars.is_empty() {
            result.push_str("\nRelevant environment:\n");
            for (key, value) in &self.env_vars {
                result.push_str(&format!("  {}={}\n", key, value));
            }
        }
        
        result
    }
}

/// Truncate output to a maximum size, keeping the last N bytes.
/// Adds ellipsis if truncated.
fn truncate_output(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        output.to_string()
    } else {
        // Take last max_bytes characters (might cut in middle of UTF-8 char, so be careful)
        let truncated = if let Some((idx, _)) = output
            .char_indices()
            .rev()
            .find(|(i, _)| output.len() - i <= max_bytes)
        {
            &output[idx..]
        } else {
            output
        };
        format!("...\n{}", truncated)
    }
}
