//! Command history tracking and storage.
//!
//! This module maintains a history of executed commands, allowing the AI
//! to provide suggestions based on recent command patterns and context.

/// Maximum number of commands to keep in history.
const MAX_HISTORY_SIZE: usize = 1000;

#[derive(Clone, Debug)]
pub struct History {
    commands: Vec<String>,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Add a command to history.
    /// Skips empty commands and duplicates of the last command.
    pub fn push(&mut self, cmd: String) {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return;
        }
        
        // Skip if it's a duplicate of the last command
        if self.commands.last().map(|s| s.as_str()) == Some(trimmed) {
            return;
        }
        
        self.commands.push(trimmed.to_string());
        
        // Trim history if it exceeds max size
        if self.commands.len() > MAX_HISTORY_SIZE {
            self.commands.remove(0);
        }
    }

    /// Get the most recent n commands.
    pub fn recent(&self, n: usize) -> Vec<String> {
        let len = self.commands.len();
        let start = len.saturating_sub(n);
        self.commands[start..].to_vec()
    }

    /// Get total number of commands in history.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Check if history is empty.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}
