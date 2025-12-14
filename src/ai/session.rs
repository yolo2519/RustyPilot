//! AI session management for tracking conversation history.
//!
//! This module manages multiple AI chat sessions, allowing users to maintain
//! separate conversation contexts and retrieve previous suggestions and interactions.

use std::collections::HashMap;
use std::path::PathBuf;

use async_openai::types::{
    ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use futures::StreamExt;
use tokio::sync::mpsc::{Sender, UnboundedSender};

use crate::context::ContextSnapshot;
use crate::event::{AiStreamData, AppEvent};

use super::client::AiCommandSuggestion;
use super::persistence::{
    self, PersistedMessage, PersistedSession, PersistedSessionState,
};
use super::parser;
use super::prompt;

pub type SessionId = u64;

const MAX_HISTORY_MESSAGES: usize = 50;

/// Represents a single AI chat session with conversation history
pub struct AiSession {
    pub id: SessionId,
    /// Full conversation history for OpenAI API (includes system, user, assistant messages)
    pub conversation_history: Vec<ChatCompletionRequestMessage>,
    /// Persistable conversation representation (UI-friendly content + model content).
    pub persisted_conversation: Vec<PersistedMessage>,
    /// Last command suggestion received from AI
    pub last_suggestion: Option<AiCommandSuggestion>,
    /// Accumulated response text from current streaming request
    pub current_response: String,
}

impl AiSession {
    fn new(id: SessionId, system_prompt: String) -> Self {
        let system_prompt_for_persist = system_prompt.clone();
        let system_msg = ChatCompletionRequestSystemMessageArgs::default()
            .content(system_prompt)
            .build()
            .expect("Failed to build system message")
            .into();

        Self {
            id,
            conversation_history: vec![system_msg],
            persisted_conversation: vec![PersistedMessage {
                role: "system".to_string(),
                content: system_prompt_for_persist.clone(),
                model_content: Some(system_prompt_for_persist),
            }],
            last_suggestion: None,
            current_response: String::new(),
        }
    }

    fn from_persisted(id: SessionId, mut conversation: Vec<PersistedMessage>) -> Self {
        // Ensure system message exists as first entry.
        if conversation
            .first()
            .map(|m| m.role.as_str() != "system")
            .unwrap_or(true)
        {
            let system_prompt = AiSessionManager::default_system_prompt();
            conversation.insert(
                0,
                PersistedMessage {
                    role: "system".to_string(),
                    content: system_prompt.clone(),
                    model_content: Some(system_prompt),
                },
            );
        }

        let conversation_history = conversation
            .iter()
            .filter_map(|m| persisted_to_openai_msg(m))
            .collect::<Vec<_>>();

        Self {
            id,
            conversation_history,
            persisted_conversation: conversation,
            last_suggestion: None,
            current_response: String::new(),
        }
    }
}

fn persisted_to_openai_msg(m: &PersistedMessage) -> Option<ChatCompletionRequestMessage> {
    let model_content = m.model_content.as_deref().unwrap_or(&m.content).to_string();
    match m.role.as_str() {
        "system" => ChatCompletionRequestSystemMessageArgs::default()
            .content(model_content)
            .build()
            .ok()
            .map(Into::into),
        "user" => ChatCompletionRequestUserMessageArgs::default()
            .content(model_content)
            .build()
            .ok()
            .map(Into::into),
        "assistant" => ChatCompletionRequestAssistantMessageArgs::default()
            .content(model_content)
            .build()
            .ok()
            .map(Into::into),
        _ => None,
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
    persistence_path: PathBuf,
}

impl AiSessionManager {
    pub fn new(
        ai_stream_tx: Sender<AiStreamData>,
        app_event_tx: UnboundedSender<AppEvent>,
        model: impl Into<String>,
    ) -> Self {
        Self::new_persistent(ai_stream_tx, app_event_tx, model, None)
    }

    pub fn new_persistent(
        ai_stream_tx: Sender<AiStreamData>,
        app_event_tx: UnboundedSender<AppEvent>,
        model: impl Into<String>,
        persistence_path: Option<PathBuf>,
    ) -> Self {
        let system_prompt = Self::default_system_prompt();
        let persistence_path = persistence_path.unwrap_or_else(persistence::default_sessions_path);
        let mut manager = Self {
            sessions: HashMap::new(),
            current_id: 1,
            next_id: 2,
            ai_stream_tx,
            app_event_tx,
            client: Client::new(),
            model: model.into(),
            persistence_path,
        };

        // Best-effort load. If anything fails, fall back to a single new session.
        match persistence::load(&manager.persistence_path) {
            Ok(state) => manager.apply_persisted_state(state),
            Err(_) => {
                manager.sessions.insert(1, AiSession::new(1, system_prompt));
            }
        }

        manager
    }

    /// Default system prompt for shell command assistance
    fn default_system_prompt() -> String {
        r#"You are an expert shell command assistant. Your role is to help users by:

1. Understanding their natural language requests
2. Suggesting safe, correct shell commands
3. Explaining what the commands do
4. Providing alternatives when appropriate

When suggesting commands:
- Always explain what the command does
- Warn about potentially dangerous operations
- Consider the user's current directory and environment
- Prefer standard POSIX commands when possible
- Format your response clearly

If you suggest a command, format it like this:
COMMAND: <the actual command>
EXPLANATION: <what it does and why>
ALTERNATIVES: <optional alternative commands, one per line>

Be concise but thorough. Safety first."#
            .to_string()
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
            self.save_best_effort();
            true
        } else {
            false
        }
    }

    pub fn session_ids(&self) -> Vec<SessionId> {
        let mut ids = self.sessions.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    pub fn session_exists(&self, session_id: SessionId) -> bool {
        self.sessions.contains_key(&session_id)
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

    pub fn new_session(&mut self) -> SessionId {
        let id = self.next_id;
        self.next_id += 1;
        let system_prompt = Self::default_system_prompt();
        self.sessions.insert(id, AiSession::new(id, system_prompt));
        self.current_id = id;
        self.save_best_effort();
        id
    }

    /// Manually close (delete) a session.
    ///
    /// If the session is currently active, the manager will switch to another
    /// remaining session. If it was the last session, a new empty session is created.
    pub fn close_session(&mut self, session_id: SessionId) -> bool {
        if !self.sessions.contains_key(&session_id) {
            return false;
        }

        self.sessions.remove(&session_id);

        if self.sessions.is_empty() {
            // Always ensure at least one session exists.
            let id = 1;
            self.current_id = id;
            self.next_id = 2;
            let system_prompt = Self::default_system_prompt();
            self.sessions.insert(id, AiSession::new(id, system_prompt));
        } else if self.current_id == session_id {
            // Switch to the smallest existing session id.
            if let Some(next) = self.session_ids().first().copied() {
                self.current_id = next;
            }
        }

        // Keep next_id monotonic.
        if let Some(max_id) = self.sessions.keys().copied().max() {
            self.next_id = self.next_id.max(max_id + 1);
        }

        self.save_best_effort();
        true
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
        // Keep current_id aligned with the UI.
        self.current_id = session_id;

        // Build prompt with context
        let prompt = prompt::build_prompt(user_input, &context);

        // Mutate session history, then clone messages for the request (to avoid borrow issues).
        let messages_for_request = {
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

            // Create user message
            let user_msg = match ChatCompletionRequestUserMessageArgs::default()
                .content(prompt.clone())
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
            session.persisted_conversation.push(PersistedMessage {
                role: "user".to_string(),
                content: user_input.to_string(),
                model_content: Some(prompt.clone()),
            });
            Self::trim_history(session);
            session.conversation_history.clone()
        };

        // Persist after mutating state (best-effort).
        self.save_best_effort();

        // Build OpenAI request
        let request = match CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(messages_for_request)
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
        let _event_tx = self.app_event_tx.clone();
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
            session.persisted_conversation.push(PersistedMessage {
                role: "assistant".to_string(),
                content: response.clone(),
                model_content: None,
            });

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
            self.save_best_effort();
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
        if session.conversation_history.len() <= MAX_HISTORY_MESSAGES
            && session.persisted_conversation.len() <= MAX_HISTORY_MESSAGES
        {
            return;
        }

        fn trim_vec<T: Clone>(v: &mut Vec<T>) {
            if v.len() <= MAX_HISTORY_MESSAGES {
                return;
            }
            let mut new_v = Vec::with_capacity(MAX_HISTORY_MESSAGES);
            if let Some(first) = v.first() {
                new_v.push(first.clone());
            }
            let to_keep = v
                .iter()
                .rev()
                .take(MAX_HISTORY_MESSAGES.saturating_sub(1))
                .cloned()
                .collect::<Vec<_>>();
            for item in to_keep.into_iter().rev() {
                new_v.push(item);
            }
            *v = new_v;
        }

        // Always keep system prompt at index 0.
        trim_vec(&mut session.conversation_history);
        trim_vec(&mut session.persisted_conversation);
    }

    fn build_persisted_state(&self) -> PersistedSessionState {
        let mut sessions = self
            .sessions
            .values()
            .map(|s| PersistedSession {
                id: s.id,
                conversation: s.persisted_conversation.clone(),
                last_suggestion: s.last_suggestion.clone(),
            })
            .collect::<Vec<_>>();
        sessions.sort_by_key(|s| s.id);

        PersistedSessionState {
            version: 1,
            current_id: self.current_id,
            next_id: self.next_id,
            sessions,
        }
    }

    fn apply_persisted_state(&mut self, state: PersistedSessionState) {
        self.sessions.clear();

        for ps in state.sessions {
            let mut session = AiSession::from_persisted(ps.id, ps.conversation);
            session.last_suggestion = ps.last_suggestion;
            Self::trim_history(&mut session);
            self.sessions.insert(ps.id, session);
        }

        if self.sessions.is_empty() {
            let system_prompt = Self::default_system_prompt();
            self.sessions.insert(1, AiSession::new(1, system_prompt));
            self.current_id = 1;
            self.next_id = 2;
            return;
        }

        self.current_id = if self.sessions.contains_key(&state.current_id) {
            state.current_id
        } else {
            *self.session_ids().first().unwrap()
        };

        // next_id should be monotonic and at least max+1.
        let max_id = self.sessions.keys().copied().max().unwrap_or(1);
        self.next_id = state.next_id.max(max_id + 1);
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let state = self.build_persisted_state();
        persistence::save(&self.persistence_path, &state)
    }

    pub fn save_best_effort(&self) {
        if let Err(e) = self.save() {
            eprintln!("Failed to persist AI sessions: {e:#}");
        }
    }

    pub fn persistence_path(&self) -> &std::path::Path {
        &self.persistence_path
    }

    /// Returns UI-friendly messages for a session (skipping system prompt).
    pub fn ui_messages(&self, session_id: SessionId) -> Vec<(String, String)> {
        let Some(session) = self.sessions.get(&session_id) else {
            return vec![];
        };

        session
            .persisted_conversation
            .iter()
            .skip(1) // system
            .filter_map(|m| match m.role.as_str() {
                "user" => Some(("user".to_string(), m.content.clone())),
                "assistant" => Some(("assistant".to_string(), m.content.clone())),
                _ => None,
            })
            .collect()
    }
}
