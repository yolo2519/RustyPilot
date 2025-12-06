//! AI session management for tracking conversation history.
//!
//! This module manages multiple AI chat sessions, allowing users to maintain
//! separate conversation contexts and retrieve previous suggestions and interactions.

use std::collections::HashMap;

use async_openai::error::OpenAIError;
use async_openai::types::{
    ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use futures::StreamExt;
use tokio::sync::mpsc::{Sender, UnboundedSender};
use tracing::error;

use crate::context::ContextSnapshot;
use crate::event::{AiStreamData, AppEvent};

use super::client::AiCommandSuggestion;

use super::parser;
use super::prompt;

pub type SessionId = u64;

const MAX_HISTORY_MESSAGES: usize = 50;

/// Represents a single AI chat session with conversation history
pub struct AiSession {
    pub id: SessionId,
    /// Full conversation history for OpenAI API (includes system, user, assistant messages)
    pub conversation_history: Vec<ChatCompletionRequestMessage>,
    /// Last command suggestion received from AI
    pub last_suggestion: Option<AiCommandSuggestion>,
    /// Accumulated response text from current streaming request
    pub current_response: String,
}

impl AiSession {
    fn new(id: SessionId, system_prompt: String) -> Result<Self, OpenAIError> {
        let system_msg = ChatCompletionRequestSystemMessageArgs::default()
            .content(system_prompt)
            .build()?
            .into();

        Ok(Self {
            id,
            conversation_history: vec![system_msg],
            last_suggestion: None,
            current_response: String::new(),
        })
    }
}

/// Manages multiple AI sessions and handles communication with OpenAI
pub struct AiSessionManager {
    sessions: HashMap<SessionId, AiSession>,
    current_id: SessionId,
    next_id: SessionId,
    ai_stream_tx: Sender<AiStreamData>,
    app_event_tx: UnboundedSender<AppEvent>,
    client: Client<async_openai::config::OpenAIConfig>,
    model: String,
}

impl AiSessionManager {
    pub fn new(
        ai_stream_tx: Sender<AiStreamData>,
        app_event_tx: UnboundedSender<AppEvent>,
        model: impl Into<String>,
    ) -> Result<Self, OpenAIError> {
        let system_prompt = prompt::SYSTEM_PROMPT.to_string();
        let mut manager = Self {
            sessions: HashMap::new(),
            current_id: 1,
            next_id: 2,
            ai_stream_tx,
            app_event_tx,
            client: Client::new(),
            model: model.into(),
        };
        manager.sessions.insert(1, AiSession::new(1, system_prompt)?);
        Ok(manager)
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

    pub fn switch_session(&mut self, session_id: SessionId) -> bool {
        if self.sessions.contains_key(&session_id) {
            self.current_id = session_id;
            true
        } else {
            false
        }
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
        if self.get_last_suggestion(session_id).is_some() {
            self.app_event_tx
                .send(AppEvent::ExecuteAiCommand { session_id })?;
        }
        Ok(())
    }

    pub fn new_session(&mut self) -> Result<SessionId, OpenAIError> {
        let id = self.next_id;
        self.next_id += 1;
        let system_prompt = prompt::SYSTEM_PROMPT.to_string();
        self.sessions.insert(id, AiSession::new(id, system_prompt)?);
        self.current_id = id;
        Ok(id)
    }

    /// Send a message to the AI with system context and receive a streaming response.
    ///
    /// This method:
    /// 1. Builds a prompt with user query + system context (cwd, env, history)
    /// 2. Sends the request to OpenAI with streaming enabled
    /// 3. Streams chunks back through the ai_stream_tx channel
    /// 4. Parses the final response for command suggestions
    /// 5. Sends command suggestion events through app_event_tx
    pub fn send_message(
        &mut self,
        session_id: SessionId,
        user_input: &str,
        context: ContextSnapshot,
    ) {
        // Get session and add user message to history
        let session = match self.sessions.get_mut(&session_id) {
            Some(s) => s,
            None => {
                let _ = self.ai_stream_tx.try_send(AiStreamData::Error {
                    session_id,
                    error: "Session not found".to_string(),
                });
                return;
            }
        };

        // Build prompt with context
        let prompt = prompt::build_prompt(user_input, &context);

        // Create user message
        let user_msg = match ChatCompletionRequestUserMessageArgs::default()
            .content(prompt)
            .build()
        {
            Ok(msg) => msg.into(),
            Err(e) => {
                let _ = self.ai_stream_tx.try_send(AiStreamData::Error {
                    session_id,
                    error: format!("Failed to build message: {}", e),
                });
                return;
            }
        };

        session.conversation_history.push(user_msg);
        session.current_response.clear();
        Self::trim_history(session);

        // Build OpenAI request
        let request = match CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(session.conversation_history.clone())
            .build()
        {
            Ok(req) => req,
            Err(e) => {
                let _ = self.ai_stream_tx.try_send(AiStreamData::Error {
                    session_id,
                    error: format!("Failed to build request: {}", e),
                });
                return;
            }
        };

        // Clone what we need for the async task
        let stream_tx = self.ai_stream_tx.clone();
        let client = self.client.clone();

        // Spawn async task to handle streaming
        tokio::spawn(async move {
            match client.chat().create_stream(request).await {
                Ok(mut stream) => {
                    // Process streaming chunks
                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(response) => {
                                for choice in response.choices {
                                    if let Some(content) = choice.delta.content {
                                        // Send chunk to UI
                                        let _ = stream_tx
                                            .send(AiStreamData::Chunk {
                                                session_id,
                                                text: content,
                                            })
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = stream_tx
                                    .send(AiStreamData::Error {
                                        session_id,
                                        error: format!("Stream error: {}", e),
                                    })
                                    .await;
                                return;
                            }
                        }
                    }

                    // Stream completed successfully
                    let _ = stream_tx.send(AiStreamData::End { session_id }).await;
                }
                Err(e) => {
                    let _ = stream_tx
                        .send(AiStreamData::Error {
                            session_id,
                            error: format!("API error: {}", e),
                        })
                        .await;
                }
            }
        });
    }

    /// Update session with the completed AI response
    /// Call this when receiving AiStreamData::End
    pub fn finalize_response(&mut self, session_id: SessionId, response: String) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            // Add assistant message to conversation history
            if let Ok(assistant_msg) = ChatCompletionRequestAssistantMessageArgs::default()
                .content(response.clone())
                .build()
            {
                session.conversation_history.push(assistant_msg.into());
            }

            // Parse and store command suggestion
            if let Some(suggestion) = parser::parse_command_suggestion(&response) {
                session.last_suggestion = Some(suggestion.clone());
                // Emit suggestion event so UI can render a card
                let _ = self.app_event_tx.send(AppEvent::AiCommandSuggestion {
                    session_id,
                    command: suggestion.suggested_command,
                    explanation: suggestion.natural_language_explanation,
                });
            }

            // Clear current response buffer
            session.current_response.clear();
            Self::trim_history(session);
        }
    }

    /// Append a chunk to the current response being streamed
    pub fn append_chunk(&mut self, session_id: SessionId, chunk: &str) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.current_response.push_str(chunk);
        }
    }

    /// Get the current accumulated response for a session
    pub fn get_current_response(&self, session_id: SessionId) -> Option<&str> {
        self.sessions
            .get(&session_id)
            .map(|s| s.current_response.as_str())
    }

    fn trim_history(session: &mut AiSession) {
        if session.conversation_history.len() <= MAX_HISTORY_MESSAGES {
            return;
        }

        // Always keep the initial system prompt
        let mut new_history = Vec::with_capacity(MAX_HISTORY_MESSAGES);
        if let Some(first) = session.conversation_history.first() {
            new_history.push(first.clone());
        }

        let to_keep = session
            .conversation_history
            .iter()
            .rev()
            .take(MAX_HISTORY_MESSAGES.saturating_sub(1))
            .cloned()
            .collect::<Vec<_>>();

        for msg in to_keep.into_iter().rev() {
            new_history.push(msg);
        }

        session.conversation_history = new_history;
    }
}
