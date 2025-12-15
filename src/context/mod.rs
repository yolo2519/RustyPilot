//! Context management for capturing shell environment state.
//!
//! This module collects and provides contextual information about the current
//! shell session including working directory, environment variables, and command
//! history to enhance AI command suggestions.

mod command_log;
mod cwd;
mod env;
mod history;

use serde::{Deserialize, Serialize};

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
            // Only take last 6 lines for AI prompt
            recent_output: self.recent_output.iter().rev().take(6).rev().cloned().collect(),
            recent_commands: Vec::new(), // Filled by caller with ShellManager data
        }
    }

    /// Create a snapshot with command records from ShellManager.
    /// Truncates command outputs to 2KB for AI prompt efficiency.
    pub fn snapshot_with_commands(&self, command_records: Vec<CommandRecord>) -> ContextSnapshot {
        // Truncate command outputs to reasonable size
        let truncated_commands: Vec<CommandRecord> = command_records
            .into_iter()
            .map(|mut record| {
                if record.output.len() > 2048 {
                    record.output = truncate_output(&record.output, 2048);
                }
                record
            })
            .collect();

        ContextSnapshot {
            cwd: self.cwd.path.clone(),
            env_vars: self.env.filtered_vars(),
            recent_history: self.history.recent(20),
            // Only take last 6 lines for AI prompt
            recent_output: self.recent_output.iter().rev().take(6).rev().cloned().collect(),
            recent_commands: truncated_commands,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub cwd: String,
    #[serde(rename = "env", skip_serializing_if = "Vec::is_empty", default)]
    pub env_vars: Vec<(String, String)>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub recent_history: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub recent_output: Vec<String>,
    /// Recent commands with their outputs (command_line, output)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub recent_commands: Vec<CommandRecord>,
}

/// Truncate output to a maximum size, keeping the last N bytes.
/// Preserves UTF-8 boundaries and adds ellipsis if truncated.
fn truncate_output(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        return output.to_string();
    }

    // Find a valid UTF-8 boundary within max_bytes from the end
    let start_from = output.len().saturating_sub(max_bytes);
    let truncated = if let Some((idx, _)) = output[start_from..]
        .char_indices()
        .next()
    {
        &output[start_from + idx..]
    } else {
        &output[start_from..]
    };

    format!("...[truncated]\n{}", truncated)
}
