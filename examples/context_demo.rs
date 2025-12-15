//! Demonstration of the context capture system.
//!
//! This example shows how to use ContextManager and CommandRecord for context snapshots.

use rusty_term::context::{CommandRecord, ContextManager, ContextSnapshot};

/// Format a context snapshot as human-readable text for display.
fn format_snapshot(snapshot: &ContextSnapshot) -> String {
    let mut result = String::new();

    result.push_str(&format!("Current directory: {}\n", snapshot.cwd));

    if !snapshot.recent_commands.is_empty() {
        result.push_str("\nRecent commands and outputs:\n");
        for record in &snapshot.recent_commands {
            result.push_str(&format!("\n$ {}\n", record.command_line));
            // Truncate output to reasonable size
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
    println!("=== Context Capture Demo ===\n");

    // Create a context manager
    let mut ctx_mgr = ContextManager::new();

    // Simulate some shell activity
    println!("1. Simulating shell activity...");
    ctx_mgr.update_cwd("/home/user/projects/rusty-term".to_string());
    ctx_mgr.add_to_history("cargo build".to_string());
    ctx_mgr.add_to_history("cargo test".to_string());
    ctx_mgr.push_output("Compiling rusty-term v0.1.0".to_string());
    ctx_mgr.push_output("Finished dev [unoptimized + debuginfo] target(s)".to_string());

    // Baseline mode: Snapshot without command records
    println!("\n2. Baseline Mode (without command records):");
    let snapshot = ctx_mgr.snapshot();
    println!("   CWD: {}", snapshot.cwd);
    println!("   Recent history: {:?}", snapshot.recent_history);
    println!("   Recent output lines: {}", snapshot.recent_output.len());
    println!("   Recent commands: {}", snapshot.recent_commands.len());

    // Advanced mode: With command records (simulating shell integration)
    println!("\n3. Advanced Mode (with command records):");

    // Simulate command execution records
    let cmd1 = CommandRecord::new(
        "cargo build".to_string(),
        "   Compiling rusty-term v0.1.0\n   Finished dev [unoptimized + debuginfo]".to_string(),
    );

    let cmd2 = CommandRecord::new(
        "cargo test".to_string(),
        "   Running 5 tests\n   test result: ok. 5 passed".to_string(),
    );

    let snapshot_with_cmds = ctx_mgr.snapshot_with_commands(vec![cmd1, cmd2]);

    println!("   Recent commands with outputs: {}", snapshot_with_cmds.recent_commands.len());
    for (i, cmd_ctx) in snapshot_with_cmds.recent_commands.iter().enumerate() {
        println!("   {}. Command: {}", i + 1, cmd_ctx.command_line);
        println!("      Output: {} chars", cmd_ctx.output.len());
    }

    // Format for AI prompt
    println!("\n4. Formatted for AI prompt:");
    println!("{}", format_snapshot(&snapshot_with_cmds));

    println!("\n=== Usage Patterns ===\n");
    println!("In your application:");
    println!("  // Simple way (without command records):");
    println!("  let snapshot = context_manager.snapshot();");
    println!();
    println!("  // With command records (from ShellManager):");
    println!("  let command_records = shell_manager.recent_commands(10);");
    println!("  let snapshot = context_manager.snapshot_with_commands(command_records);");
}
