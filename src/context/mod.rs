//! Context management for capturing shell environment state.
//!
//! This module collects and provides contextual information about the current
//! shell session including working directory, environment variables, and command
//! history to enhance AI command suggestions.

mod cwd;
mod env;
mod history;

pub use cwd::CurrentDir;
pub use env::Environment;
pub use history::History;

pub struct ContextManager {
    pub env: Environment,
    pub cwd: CurrentDir,
    pub history: History,
    recent_output: std::collections::VecDeque<String>,
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

    pub fn snapshot(&self) -> ContextSnapshot {
        ContextSnapshot {
            cwd: self.cwd.path.clone(),
            env_vars: self.env.vars.clone(),
            recent_history: self.history.recent(20),
            recent_output: self.recent_output.iter().cloned().collect(),
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
}

#[derive(Clone, Debug)]
pub struct ContextSnapshot {
    pub cwd: String,
    pub env_vars: Vec<(String, String)>,
    pub recent_history: Vec<String>,
    pub recent_output: Vec<String>,
}
