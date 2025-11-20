//! Command history tracking and storage.
//! 
//! This module maintains a history of executed commands, allowing the AI
//! to provide suggestions based on recent command patterns and context.

#[derive(Clone)]
pub struct History {
    commands: Vec<String>,
}

impl History {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, cmd: String) {
        self.commands.push(cmd);
    }

    pub fn recent(&self, n: usize) -> Vec<String> {
        let len = self.commands.len();
        let start = len.saturating_sub(n);
        self.commands[start..].to_vec()
    }
}
