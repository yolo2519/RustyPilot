//! Environment variable capture and management.
//!
//! This module captures the current environment variables from the shell
//! session, which can be used to provide better context for AI suggestions.

/// Environment variables that are useful for AI context.
const RELEVANT_ENV_VARS: &[&str] = &[
    "SHELL",
    "USER",
    "HOME",
    "LANG",
    "EDITOR",
    "VISUAL",
    "TERM",
    "PATH",
    "VIRTUAL_ENV",
    "CONDA_DEFAULT_ENV",
    "NODE_ENV",
    "RUST_BACKTRACE",
    "GOPATH",
    "JAVA_HOME",
    "PYTHONPATH",
];

#[derive(Clone, Debug)]
pub struct Environment {
    pub vars: Vec<(String, String)>,
}

impl Environment {
    /// Capture all environment variables from the current process.
    pub fn capture() -> Self {
        let vars = std::env::vars().collect();
        Self { vars }
    }

    /// Get a filtered list of environment variables that are relevant for AI context.
    /// This avoids sending sensitive or irrelevant data to the AI.
    pub fn filtered_vars(&self) -> Vec<(String, String)> {
        self.vars
            .iter()
            .filter(|(key, _)| {
                RELEVANT_ENV_VARS.contains(&key.as_str())
                    || key.starts_with("GIT_")
                    || key.starts_with("npm_")
                    || key.starts_with("CARGO_")
            })
            .map(|(k, v)| {
                // Truncate long values (like PATH)
                let truncated = if v.len() > 100 {
                    format!("{}...", &v[..100])
                } else {
                    v.clone()
                };
                (k.clone(), truncated)
            })
            .collect()
    }

    /// Get a specific environment variable.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}
