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
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            env: Environment::capture(),
            cwd: CurrentDir::capture().unwrap_or_default(),
            history: History::new(),
        }
    }

    pub fn snapshot(&self) -> ContextSnapshot {
        ContextSnapshot {
            cwd: self.cwd.path.clone(),
            env_vars: self.env.vars.clone(),
            recent_history: self.history.recent(20),
        }
    }
}

#[derive(Clone)]
pub struct ContextSnapshot {
    pub cwd: String,
    pub env_vars: Vec<(String, String)>,
    pub recent_history: Vec<String>,
}
