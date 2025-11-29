//! Environment variable capture and management.
//!
//! This module captures the current environment variables from the shell
//! session, which can be used to provide better context for AI suggestions.

#[derive(Clone)]
pub struct Environment {
    pub vars: Vec<(String, String)>,
}

impl Environment {
    pub fn capture() -> Self {
        let vars = std::env::vars().collect();
        Self { vars }
    }
}
