//! Tests for context management and shell integration.

#[cfg(test)]
mod tests {
    use crate::context::{CommandContext, ContextManager};

    #[test]
    fn test_baseline_mode() {
        let mut ctx = ContextManager::new();
        
        // Add some basic context
        ctx.update_cwd("/home/user/projects".to_string());
        ctx.add_to_history("ls -la".to_string());
        ctx.push_output("file1.txt\nfile2.txt\n".to_string());
        
        let snapshot = ctx.snapshot();
        
        assert_eq!(snapshot.cwd, "/home/user/projects");
        assert_eq!(snapshot.recent_history.len(), 1);
        assert_eq!(snapshot.recent_output.len(), 2);
        assert_eq!(snapshot.recent_commands.len(), 0); // No shell integration
    }

    #[test]
    fn test_advanced_mode_with_shell_integration() {
        let mut ctx = ContextManager::new();
        
        // Simulate shell integration: command start -> output -> command end
        ctx.on_command_start("cargo build".to_string());
        ctx.push_output("Compiling rusty-term\n".to_string());
        ctx.push_output("Finished dev target\n".to_string());
        ctx.on_command_end();
        
        ctx.on_command_start("cargo test".to_string());
        ctx.push_output("Running 5 tests\n".to_string());
        ctx.on_command_end();
        
        let snapshot = ctx.snapshot();
        
        assert_eq!(snapshot.recent_commands.len(), 2);
        assert_eq!(snapshot.recent_commands[0].command_line, "cargo build");
        assert!(snapshot.recent_commands[0].output.contains("Compiling rusty-term"));
        assert_eq!(snapshot.recent_commands[1].command_line, "cargo test");
    }

    #[test]
    fn test_command_output_size_limit() {
        let mut ctx = ContextManager::new();
        
        // Generate large output
        let large_output = "x".repeat(20000);
        
        ctx.on_command_start("echo large".to_string());
        ctx.push_output(large_output);
        ctx.on_command_end();
        
        let snapshot = ctx.snapshot();
        
        // Output should be truncated
        assert!(snapshot.recent_commands[0].output.len() <= 5100); // ~5000 + truncation marker
    }

    #[test]
    fn test_max_commands_limit() {
        let mut ctx = ContextManager::new();
        
        // Add more than MAX_COMMAND_CONTEXTS commands
        for i in 0..15 {
            ctx.on_command_start(format!("command_{}", i));
            ctx.push_output(format!("output_{}\n", i));
            ctx.on_command_end();
        }
        
        let snapshot = ctx.snapshot();
        
        // Should only keep last 10 commands
        assert!(snapshot.recent_commands.len() <= 10);
        // Should have the most recent commands
        assert_eq!(snapshot.recent_commands.last().unwrap().command_line, "command_14");
    }

    #[test]
    fn test_osc7_cwd_update() {
        let mut ctx = ContextManager::new();
        
        // OSC 7 format: file://hostname/path
        let result = ctx.update_cwd_from_osc7("file://localhost/home/user/projects");
        
        assert!(result);
        assert_eq!(ctx.cwd.path, "/home/user/projects");
    }

    #[test]
    fn test_format_for_prompt_with_commands() {
        let mut ctx = ContextManager::new();
        ctx.update_cwd("/test".to_string());
        
        ctx.on_command_start("ls".to_string());
        ctx.push_output("file1\nfile2\nfile3\n".to_string());
        ctx.on_command_end();
        
        let snapshot = ctx.snapshot();
        let formatted = snapshot.format_for_prompt();
        
        assert!(formatted.contains("Current directory: /test"));
        assert!(formatted.contains("Recent commands with outputs:"));
        assert!(formatted.contains("Command: ls"));
        assert!(formatted.contains("Output:"));
        assert!(formatted.contains("file1"));
    }

    #[test]
    fn test_format_for_prompt_baseline() {
        let mut ctx = ContextManager::new();
        ctx.update_cwd("/test".to_string());
        ctx.add_to_history("echo hello".to_string());
        ctx.push_output("hello world\n".to_string());
        
        let snapshot = ctx.snapshot();
        let formatted = snapshot.format_for_prompt();
        
        // Without shell integration, should show history and output separately
        assert!(formatted.contains("Current directory: /test"));
        assert!(formatted.contains("Recent commands:") || formatted.contains("Recent terminal output:"));
    }

    #[test]
    fn test_command_context_clone() {
        let cmd_ctx = CommandContext {
            command_line: "test".to_string(),
            output: "output".to_string(),
        };
        
        let cloned = cmd_ctx.clone();
        assert_eq!(cloned.command_line, "test");
        assert_eq!(cloned.output, "output");
    }
}
