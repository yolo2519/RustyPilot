//! Demonstration of the AI Session Manager with Tool Calling
//!
//! This example shows how to use the AI Session Manager to:
//! - Create and manage multiple chat sessions
//! - Send messages with system context
//! - Receive streaming responses via recv_ai_stream()
//! - Handle structured command suggestions via OpenAI Tool Calling
//! - Accept or reject suggested commands (responses added to conversation history)
//!
//! Run with: cargo run --example ai_session_demo
//!
//! Make sure to set OPENAI_API_KEY environment variable first.

use std::io::{self, Write};

use anyhow::Result;
use rusty_term::ai::AiSessionManager;
use rusty_term::context::ContextSnapshot;
use rusty_term::event::{init_app_eventsource, AiUiUpdate, AppEvent};

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== AI Session Manager Demo ===\n");
    println!("This demo shows the AI Session Manager in action.");
    println!("Make sure OPENAI_API_KEY is set in your environment.\n");

    // Check for API key
    if std::env::var("OPENAI_API_KEY").is_err() {
        eprintln!("ERROR: OPENAI_API_KEY environment variable not set!");
        eprintln!("Please set it with: export OPENAI_API_KEY='your-key-here'");
        return Ok(());
    }

    // Create app event channel
    let (app_event_tx, mut app_event_rx) = init_app_eventsource();

    // Create AI Session Manager (now owns its own stream channel internally)
    let mut session_manager = AiSessionManager::new(
        app_event_tx,
        "gpt-4o-mini", // Use a real model name
    )?;

    println!("✓ AI Session Manager initialized");
    println!("✓ Session ID: {}\n", session_manager.current_session_id());

    // Create a mock context snapshot
    let context = ContextSnapshot {
        cwd: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "/unknown".to_string()),
        env_vars: vec![
            ("HOME".to_string(), std::env::var("HOME").unwrap_or_default()),
            ("SHELL".to_string(), std::env::var("SHELL").unwrap_or_default()),
            ("USER".to_string(), std::env::var("USER").unwrap_or_default()),
        ],
        recent_history: vec![
            "cargo build".to_string(),
            "cargo test".to_string(),
            "git status".to_string(),
        ],
        recent_output: vec![],
    };

    println!("Context:");
    println!("  Working Directory: {}", context.cwd);
    println!("  Recent Commands: {:?}\n", context.recent_history);

    // Interactive loop
    loop {
        print!("Enter your request (or 'quit' to exit): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input == "quit" || input == "exit" {
            println!("Goodbye!");
            break;
        }

        // Handle special commands
        if input == "new" {
            let new_id = session_manager.new_session()?;
            println!("✓ Created new session: {}\n", new_id);
            continue;
        }

        if input.starts_with("switch ") {
            if let Ok(id) = input[7..].parse::<u64>() {
                if session_manager.switch_session(id) {
                    println!("✓ Switched to session: {}\n", id);
                } else {
                    println!("✗ Session {} not found\n", id);
                }
            }
            continue;
        }

        // Send message to AI
        println!("\n--- Sending to AI ---");
        let session_id = session_manager.current_session_id();
        session_manager.send_message(session_id, input, context.clone());

        // Process streaming response using recv_ai_stream()
        println!("AI Response: ");
        print!("  ");
        io::stdout().flush()?;

        let mut stream_ended = false;

        // Process stream and events
        while !stream_ended {
            tokio::select! {
                // Handle AI stream data via recv_ai_stream()
                // This is the new pattern: AiSessionManager owns the stream and
                // stores data internally, then returns AiUiUpdate for display
                Some(update) = session_manager.recv_ai_stream() => {
                    match update {
                        AiUiUpdate::Chunk { text, .. } => {
                            print!("{}", text);
                            io::stdout().flush()?;
                        }
                        AiUiUpdate::End { .. } => {
                            println!("\n");
                            stream_ended = true;
                        }
                        AiUiUpdate::Error { error, .. } => {
                            println!("\n✗ Error: {}\n", error);
                            stream_ended = true;
                        }
                        AiUiUpdate::CommandSuggestion { command, explanation, .. } => {
                            println!("\n");
                            println!("--- Command Suggestion ---");
                            println!("  Command: {}", command);
                            println!("  Explanation: {}", explanation);
                            println!();
                            stream_ended = true;
                        }
                    }
                }

                // Handle app events
                Some(app_event) = app_event_rx.recv() => {
                    match app_event {
                        AppEvent::ExecuteAiCommand { command, .. } => {
                            println!("--- Executing Command ---");
                            println!("  {}", command);
                            println!("  (In a real app, this would execute the command)");
                            println!();
                        }
                        _ => {}
                    }
                }

                // Timeout after 30 seconds
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                    println!("\n✗ Timeout waiting for response\n");
                    stream_ended = true;
                }
            }
        }

        // Show suggestion if available (from the session manager)
        if let Some(suggestion) = session_manager.get_pending_suggestion(session_id) {
            println!("--- Pending Suggestion ---");
            println!("  Command: {}", suggestion.command);
            println!("  Explanation: {}", suggestion.explanation);
            println!("  Status: {:?}", suggestion.status);
            println!();

            // Demo: Ask user to accept or reject
            print!("Accept this suggestion? (y/n): ");
            io::stdout().flush()?;
            let mut response = String::new();
            io::stdin().read_line(&mut response)?;
            let response = response.trim().to_lowercase();

            if response == "y" || response == "yes" {
                if let Some(cmd) = session_manager.accept_suggestion(session_id) {
                    println!("✓ Accepted command: {}", cmd);
                    // In real app, would execute the command here
                    session_manager.execute_suggestion(session_id, cmd)?;
                }
            } else {
                session_manager.reject_suggestion(session_id);
                println!("✗ Rejected suggestion");
            }
            println!();
        }
    }

    Ok(())
}
