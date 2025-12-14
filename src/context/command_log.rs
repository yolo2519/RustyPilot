//! Command and output logging for AI context.
//!
//! This module records user-executed commands and their outputs,
//! providing structured context to improve AI suggestions.

/// A single command execution record with its output.
#[derive(Clone, Debug)]
pub struct CommandRecord {
    pub command_line: String,
    pub output: String,
}

impl CommandRecord {
    /// Create a new command record.
    pub fn new(command_line: String, output: String) -> Self {
        Self {
            command_line,
            output,
        }
    }
}

/// Maintains a bounded log of recent command executions.
pub struct CommandLog {
    entries: Vec<CommandRecord>,
    max_len: usize,
}

impl CommandLog {
    /// Create a new command log with a maximum capacity.
    pub fn new(max_len: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max_len),
            max_len,
        }
    }

    /// Start recording a new command.
    /// Finalizes the previous command (if any) and creates a new empty record.
    pub fn start_new_command(&mut self, command_line: String) {
        // Drop oldest entry if at capacity
        if self.entries.len() >= self.max_len {
            self.entries.remove(0);
        }
        
        // Add new command with empty output
        self.entries.push(CommandRecord {
            command_line,
            output: String::new(),
        });
    }

    /// Append output data to the most recent command.
    /// Uses lossy UTF-8 conversion to handle invalid sequences.
    pub fn append_output(&mut self, bytes: &[u8]) {
        if let Some(last) = self.entries.last_mut() {
            // Convert bytes to string, replacing invalid UTF-8 sequences
            let text = String::from_utf8_lossy(bytes);
            last.output.push_str(&text);
        }
    }

    /// Add a command record to the log.
    /// If the log is full, removes the oldest entry.
    pub fn push(&mut self, record: CommandRecord) {
        if self.entries.len() >= self.max_len {
            self.entries.remove(0);
        }
        self.entries.push(record);
    }

    /// Get all command records.
    pub fn entries(&self) -> &[CommandRecord] {
        &self.entries
    }

    /// Get the most recent n command records.
    pub fn recent(&self, n: usize) -> &[CommandRecord] {
        let len = self.entries.len();
        let start = len.saturating_sub(n);
        &self.entries[start..]
    }

    /// Get the number of entries in the log.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for CommandLog {
    fn default() -> Self {
        Self::new(50) // Default to keeping last 50 commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_new_command() {
        let mut log = CommandLog::new(10);
        
        log.start_new_command("ls -la".to_string());
        assert_eq!(log.len(), 1);
        assert_eq!(log.entries()[0].command_line, "ls -la");
        assert_eq!(log.entries()[0].output, "");
    }

    #[test]
    fn test_append_output() {
        let mut log = CommandLog::new(10);
        
        log.start_new_command("echo hello".to_string());
        log.append_output(b"hello\n");
        log.append_output(b"world\n");
        
        assert_eq!(log.entries()[0].output, "hello\nworld\n");
    }

    #[test]
    fn test_bounded_log() {
        let mut log = CommandLog::new(3);
        
        log.start_new_command("cmd1".to_string());
        log.start_new_command("cmd2".to_string());
        log.start_new_command("cmd3".to_string());
        log.start_new_command("cmd4".to_string());
        
        // Should only keep last 3
        assert_eq!(log.len(), 3);
        assert_eq!(log.entries()[0].command_line, "cmd2");
        assert_eq!(log.entries()[1].command_line, "cmd3");
        assert_eq!(log.entries()[2].command_line, "cmd4");
    }

    #[test]
    fn test_recent() {
        let mut log = CommandLog::new(10);
        
        log.start_new_command("cmd1".to_string());
        log.start_new_command("cmd2".to_string());
        log.start_new_command("cmd3".to_string());
        log.start_new_command("cmd4".to_string());
        
        let recent = log.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].command_line, "cmd3");
        assert_eq!(recent[1].command_line, "cmd4");
    }

    #[test]
    fn test_recent_more_than_available() {
        let mut log = CommandLog::new(10);
        
        log.start_new_command("cmd1".to_string());
        log.start_new_command("cmd2".to_string());
        
        let recent = log.recent(10);
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_append_output_invalid_utf8() {
        let mut log = CommandLog::new(10);
        
        log.start_new_command("test".to_string());
        // Invalid UTF-8 sequence
        log.append_output(&[0xFF, 0xFE, 0xFD]);
        
        // Should handle gracefully with replacement characters
        assert!(!log.entries()[0].output.is_empty());
    }

    #[test]
    fn test_multiple_commands_with_output() {
        let mut log = CommandLog::new(10);
        
        log.start_new_command("ls".to_string());
        log.append_output(b"file1.txt\n");
        log.append_output(b"file2.txt\n");
        
        log.start_new_command("pwd".to_string());
        log.append_output(b"/home/user\n");
        
        assert_eq!(log.len(), 2);
        assert_eq!(log.entries()[0].command_line, "ls");
        assert_eq!(log.entries()[0].output, "file1.txt\nfile2.txt\n");
        assert_eq!(log.entries()[1].command_line, "pwd");
        assert_eq!(log.entries()[1].output, "/home/user\n");
    }
}
