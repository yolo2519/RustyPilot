//! Demonstrates the command context recording system.
//!
//! This example shows how CommandLog tracks commands and outputs,
//! and how this data is used for AI context.

use rusty_term::context::{CommandLog, CommandRecord, ContextManager, ContextSnapshot};

/// Format a context snapshot as human-readable text for display.
fn format_snapshot(snapshot: &ContextSnapshot) -> String {
    let mut result = String::new();

    result.push_str(&format!("Current directory: {}\n", snapshot.cwd));

    if !snapshot.recent_commands.is_empty() {
        result.push_str("\nRecent commands and outputs:\n");
        for record in &snapshot.recent_commands {
            result.push_str(&format!("\n$ {}\n", record.command_line));
            let output_preview = if record.output.len() > 2048 {
                format!("...{}", &record.output[record.output.len() - 2048..])
            } else {
                record.output.clone()
            };
            if !output_preview.is_empty() {
                result.push_str(&output_preview);
                result.push('\n');
            }
        }
    } else if !snapshot.recent_history.is_empty() {
        result.push_str("\nRecent commands:\n");
        for (i, cmd) in snapshot.recent_history.iter().rev().take(5).enumerate() {
            result.push_str(&format!("  {}. {}\n", i + 1, cmd));
        }
    }

    if !snapshot.env_vars.is_empty() {
        result.push_str("\nRelevant environment:\n");
        for (key, value) in &snapshot.env_vars {
            result.push_str(&format!("  {}={}\n", key, value));
        }
    }

    result
}

fn main() {
    println!("=== Command Context Demo ===\n");

    // Create a command log
    let mut log = CommandLog::new(10);

    // Simulate user running commands
    println!("Simulating terminal session...\n");

    // Command 1: ls
    log.start_new_command("ls -la".to_string());
    log.append_output(b"total 16\n");
    log.append_output(b"drwxr-xr-x  5 user  staff  160 Dec 13 10:00 .\n");
    log.append_output(b"-rw-r--r--  1 user  staff  100 Dec 13 09:30 README.md\n");

    // Command 2: cat
    log.start_new_command("cat README.md".to_string());
    log.append_output(b"# My Project\n");
    log.append_output(b"This is a demo project.\n");

    // Command 3: git status
    log.start_new_command("git status".to_string());
    log.append_output(b"On branch main\n");
    log.append_output(b"Your branch is up to date with 'origin/main'.\n");
    log.append_output(b"nothing to commit, working tree clean\n");

    // Display recorded commands
    println!("📝 Recorded {} commands:", log.len());
    for (i, record) in log.entries().iter().enumerate() {
        println!("\n{}. Command: {}", i + 1, record.command_line);
        println!("   Output preview: {}",
            record.output
                .lines()
                .take(2)
                .collect::<Vec<_>>()
                .join("\n   ")
        );
    }

    // Get recent commands for AI context
    println!("\n\n=== AI Context ===\n");
    let recent = log.recent(2);
    println!("Most recent {} commands for AI:", recent.len());

    for record in recent {
        println!("\n$ {}", record.command_line);
        for line in record.output.lines() {
            println!("{}", line);
        }
    }

    // Demonstrate context snapshot
    println!("\n\n=== Context Snapshot ===\n");
    let context_manager = ContextManager::new();
    let commands: Vec<CommandRecord> = log.recent(3).to_vec();
    let snapshot = context_manager.snapshot_with_commands(commands);

    println!("Current directory: {}", snapshot.cwd);
    println!("Recent commands in snapshot: {}", snapshot.recent_commands.len());

    // Show how this would be formatted for AI
    println!("\n=== Formatted for AI Prompt ===\n");
    println!("{}", format_snapshot(&snapshot));

    // Demonstrate bounded behavior
    println!("\n=== Bounded Log Behavior ===\n");
    let mut bounded_log = CommandLog::new(3);
    for i in 1..=5 {
        bounded_log.start_new_command(format!("command_{}", i));
    }

    println!("Added 5 commands to log with capacity 3");
    println!("Commands in log: {}", bounded_log.len());
    println!("Commands kept:");
    for record in bounded_log.entries() {
        println!("  - {}", record.command_line);
    }

    println!("\n✅ Demo complete!");
}
