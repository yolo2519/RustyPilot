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
