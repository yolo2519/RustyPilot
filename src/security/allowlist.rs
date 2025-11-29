//! Command allowlist management.
//!
//! This module maintains a list of allowed command prefixes that can be
//! executed without additional security warnings or blocks.

use std::collections::HashSet;

pub struct Allowlist {
    allowed_prefixes: HashSet<String>,
}

impl Allowlist {
    pub fn new() -> Self {
        Self {
            allowed_prefixes: HashSet::new(),
        }
    }

    pub fn allow_prefix(&mut self, prefix: &str) {
        self.allowed_prefixes.insert(prefix.to_string());
    }

    pub fn is_allowed(&self, cmd: &str) -> bool {
        self.allowed_prefixes
            .iter()
            .any(|p| cmd.trim_start().starts_with(p))
    }
}
