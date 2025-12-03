//! AI session management for tracking conversation history.
//!
//! This module manages multiple AI chat sessions, allowing users to maintain
//! separate conversation contexts and retrieve previous suggestions and interactions.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc::{Sender, UnboundedSender};

use crate::context::ContextSnapshot;
use crate::event::{AiStreamData, AppEvent};

use super::client::AiCommandSuggestion;
use super::prompt::build_prompt;

pub type SessionId = u64;

pub struct AiSession {
    pub id: SessionId,
    pub history: Vec<String>, // store text (easy version?)
    pub last_suggestion: Option<AiCommandSuggestion>,
}

pub struct AiSessionManager {
    sessions: HashMap<SessionId, AiSession>,
    current_id: SessionId,
    next_id: SessionId,
    ai_stream_tx: Sender<AiStreamData>,
    app_event_tx: UnboundedSender<AppEvent>,
}

impl AiSessionManager {
    pub fn new(ai_stream_tx: Sender<AiStreamData>, app_event_tx: UnboundedSender<AppEvent>) -> Self {
        let mut manager = Self {
            sessions: HashMap::new(),
            current_id: 1,
            next_id: 2,
            ai_stream_tx,
            app_event_tx,
        };
        manager.sessions.insert(
            1,
            AiSession {
                id: 1,
                history: Vec::new(),
                last_suggestion: None,
            },
        );
        manager
    }

    pub fn current_session(&self) -> Option<&AiSession> {
        self.sessions.get(&self.current_id)
    }

    pub fn current_session_mut(&mut self) -> Option<&mut AiSession> {
        self.sessions.get_mut(&self.current_id)
    }

    pub fn current_session_id(&self) -> SessionId {
        self.current_id
    }

    /// Get the last command suggestion for a session, if any
    pub fn get_last_suggestion(&self, session_id: SessionId) -> Option<&AiCommandSuggestion> {
        self.sessions
            .get(&session_id)
            .and_then(|s| s.last_suggestion.as_ref())
    }

    /// Execute the suggested command for a session.
    /// This sends an ExecuteAiCommand event to the app layer.
    pub fn execute_suggestion(&self, session_id: SessionId) -> anyhow::Result<()> {
        // Verify that there's a suggestion to execute
        if self.get_last_suggestion(session_id).is_some() {
            self.app_event_tx.send(AppEvent::ExecuteAiCommand { session_id })?;
        }
        Ok(())
    }

    pub fn new_session(&mut self) -> SessionId {
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.insert(
            id,
            AiSession {
                id,
                history: Vec::new(),
                last_suggestion: None,
            },
        );
        self.current_id = id;
        id
    }

    /// Send a message to the AI and receive a streaming response.
    /// 
    /// # Arguments
    /// * `session_id` - The session to send the message in
    /// * `user_input` - The user's query
    /// * `context` - Current shell context (cwd, env, history)
    /// 
    /// WARN: This is currently a fake implementation for debugging.
    /// It simulates streaming by sending chunks with delays.
    /// Replace with real AI API integration later.
    pub fn send_message(&mut self, session_id: SessionId, user_input: &str, context: ContextSnapshot) {
        // Store in history
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.history.push(format!("user: {}", user_input));
        }

        // Build the full prompt with context
        let full_prompt = build_prompt(user_input, &context);

        // Clone what we need for the async task
        let stream_tx = self.ai_stream_tx.clone();
        let event_tx = self.app_event_tx.clone();
        let input = user_input.to_string();
        let cwd = context.cwd.clone();

        // Spawn async task to simulate streaming response
        tokio::spawn(async move {
            // WARN: Fake streaming response for debugging
            // In real implementation, this would call the AI API with full_prompt
            let _ = full_prompt; // Suppress unused warning (will be used with real AI)
            
            let response = format!(
                "I see you're in `{}`. Based on your request \"{}\", I suggest the following command.",
                cwd, input
            );

            // Simulate streaming by sending character by character with delays
            for chunk in response.chars().collect::<Vec<_>>().chunks(3) {
                let text: String = chunk.iter().collect();
                let _ = stream_tx
                    .send(AiStreamData::Chunk {
                        session_id,
                        text,
                    })
                    .await;
                tokio::time::sleep(Duration::from_millis(30)).await;
            }

            // End the stream
            let _ = stream_tx.send(AiStreamData::End { session_id }).await;

            // After a short delay, send a command suggestion
            tokio::time::sleep(Duration::from_millis(100)).await;

            // WARN: Fake command suggestion based on input
            let (command, explanation) = generate_fake_suggestion(&input, &cwd);

            // Send as AppEvent so it creates a proper command card
            let _ = event_tx
                .send(AppEvent::AiCommandSuggestion {
                    session_id,
                    command: command.clone(),
                    explanation: explanation.clone(),
                });
        });

        // Store the suggestion in the session (do this synchronously before spawning)
        // Note: In the real implementation, this should be done after parsing the AI response
        let (cmd, exp) = generate_fake_suggestion(user_input, &context.cwd);
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.last_suggestion = Some(AiCommandSuggestion {
                natural_language_explanation: exp,
                suggested_command: cmd,
                alternatives: vec![],
            });
        }
    }
}

/// Generate a fake command suggestion based on user input.
/// This will be replaced with real AI response parsing.
fn generate_fake_suggestion(input: &str, cwd: &str) -> (String, String) {
    let input_lower = input.to_lowercase();
    
    if input_lower.contains("list") || input_lower.contains("show") || input_lower.contains("ls") {
        ("ls -la".to_string(), "List all files in the current directory with details".to_string())
    } else if input_lower.contains("find") || input_lower.contains("search") {
        ("find . -name '*pattern*' -type f".to_string(), "Search for files matching a pattern".to_string())
    } else if input_lower.contains("disk") || input_lower.contains("space") || input_lower.contains("size") {
        ("du -sh *".to_string(), "Show disk usage of files and directories".to_string())
    } else if input_lower.contains("process") || input_lower.contains("running") {
        ("ps aux | head -20".to_string(), "Show running processes".to_string())
    } else if input_lower.contains("git") && input_lower.contains("status") {
        ("git status".to_string(), "Show the working tree status".to_string())
    } else if input_lower.contains("network") || input_lower.contains("ip") {
        ("ifconfig || ip addr".to_string(), "Show network interface information".to_string())
    } else {
        (
            format!("echo 'Working in: {}'", cwd),
            format!("Echo the current working directory (you asked: {})", input),
        )
    }
}
