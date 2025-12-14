//! Demonstration of the context capture system.
//!
//! This example shows how to use both baseline and advanced context modes.

use rusty_term::context::ContextManager;

fn main() {
    println!("=== Context Capture Demo ===\n");
    
    // Create a context manager
    let mut ctx_mgr = ContextManager::new();
    
    // Simulate some shell activity
    println!("1. Simulating shell activity...");
    ctx_mgr.update_cwd("/home/user/projects/rusty-term".to_string());
    ctx_mgr.add_to_history("cargo build".to_string());
    ctx_mgr.add_to_history("cargo test".to_string());
    ctx_mgr.push_output("Compiling rusty-term v0.1.0\n".to_string());
    ctx_mgr.push_output("Finished dev [unoptimized + debuginfo] target(s)\n".to_string());
    
    // Baseline mode: Snapshot without shell integration
    println!("\n2. Baseline Mode (without shell integration):");
    let snapshot = ctx_mgr.snapshot();
    println!("   CWD: {}", snapshot.cwd);
    println!("   Recent history: {:?}", snapshot.recent_history);
    println!("   Recent output lines: {}", snapshot.recent_output.len());
    println!("   Recent commands (shell integration): {}", snapshot.recent_commands.len());
    
    // Advanced mode: Simulate shell integration
    println!("\n3. Advanced Mode (with shell integration):");
    ctx_mgr.on_command_start("cargo build".to_string());
    ctx_mgr.push_output("   Compiling rusty-term v0.1.0\n".to_string());
    ctx_mgr.push_output("   Finished dev [unoptimized + debuginfo]\n".to_string());
    ctx_mgr.on_command_end();
    
    ctx_mgr.on_command_start("cargo test".to_string());
    ctx_mgr.push_output("   Running 5 tests\n".to_string());
    ctx_mgr.push_output("   test result: ok. 5 passed\n".to_string());
    ctx_mgr.on_command_end();
    
    let snapshot_advanced = ctx_mgr.snapshot();
    println!("   Recent commands with outputs: {}", snapshot_advanced.recent_commands.len());
    for (i, cmd_ctx) in snapshot_advanced.recent_commands.iter().enumerate() {
        println!("   {}. Command: {}", i + 1, cmd_ctx.command_line);
        println!("      Output: {} chars", cmd_ctx.output.len());
    }
    
    // Format for AI prompt
    println!("\n4. Formatted for AI prompt:");
    println!("{}", snapshot_advanced.format_for_prompt());
    
    println!("\n=== Usage Patterns ===\n");
    println!("In your application:");
    println!("  // Simple way (using ContextManager):");
    println!("  let snapshot = context_manager.snapshot();");
    println!();
    println!("  // Direct collection (when you have component references):");
    println!("  let snapshot = ContextSnapshot::collect(");
    println!("      &shell_manager,");
    println!("      &tui_terminal,");
    println!("      &context_manager,");
    println!("      50,  // max output lines");
    println!("      10,  // max commands");
    println!("  );");
}
