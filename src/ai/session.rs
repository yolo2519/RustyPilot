//! AI session management for tracking conversation history.
//!
//! This module manages multiple AI chat sessions, allowing users to maintain
//! separate conversation contexts and retrieve previous suggestions and interactions.
//!
//! ## Tool Calling
//!
//! This module uses OpenAI's Tool Calling feature to get structured command suggestions.
//! The `suggest_command` tool is defined and AI will use it to suggest shell commands.

use std::collections::HashMap;

use async_openai::error::OpenAIError;
use async_openai::types::{
    ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessageArgs,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs,
    ChatCompletionTool, ChatCompletionToolType, CreateChatCompletionRequestArgs, FunctionObject,
};
use async_openai::Client;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tracing::error;

use crate::context::ContextSnapshot;
use crate::event::{AiStreamData, AiUiUpdate, AppEvent};

use super::prompt;

pub type SessionId = u64;

const MAX_HISTORY_MESSAGES: usize = 50;

// =============================================================================
// Tool Definitions
// =============================================================================

/// Name of the suggest_command tool
const TOOL_SUGGEST_COMMAND: &str = "suggest_command";

/// Arguments for the suggest_command tool (parsed from AI's JSON response)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestCommandArgs {
    /// The shell command to suggest
    pub command: String,
    /// Natural language explanation of what the command does
    pub explanation: String,
    /// Risk level of the command (low, medium, high)
    pub risk_level: String,
}

/// Create the suggest_command tool definition
fn create_suggest_command_tool() -> ChatCompletionTool {
    ChatCompletionTool {
        r#type: ChatCompletionToolType::Function,
        function: FunctionObject {
            name: TOOL_SUGGEST_COMMAND.to_string(),
            description: Some(
                "Suggest a shell command for the user to execute. \
                 Use this tool when the user asks for help with a command or task."
                    .to_string(),
            ),
            parameters: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to suggest"
                    },
                    "explanation": {
                        "type": "string",
                        "description": "A brief explanation of what the command does"
                    },
                    "risk_level": {
                        "type": "string",
                        "enum": ["low", "medium", "high"],
                        "description": "The risk level of the command (low for safe read-only commands, medium for commands that modify files, high for destructive or system-changing commands)"
                    }
                },
                "required": ["command", "explanation", "risk_level"],
                "additionalProperties": false
            })),
            strict: Some(true),
        },
    }
}

// =============================================================================
// Command Suggestion Record
// =============================================================================

/// Status of a command suggestion in the session history
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSuggestionStatus {
    /// Waiting for user decision
    Pending,
    /// User accepted and executed the command
    Accepted,
    /// User rejected the command
    Rejected,
}

/// A record of a command suggestion and its outcome
#[derive(Debug, Clone)]
pub struct CommandSuggestionRecord {
    /// The tool call ID from OpenAI (used for Tool message response)
    pub tool_call_id: String,
    /// The suggested command
    pub command: String,
    /// Explanation of what the command does
    pub explanation: String,
    /// Current status of the suggestion
    pub status: CommandSuggestionStatus,
}

// =============================================================================
// AI Session
// =============================================================================

/// Represents a single AI chat session with conversation history
pub struct AiSession {
    pub id: SessionId,
    /// Full conversation history for OpenAI API (includes system, user, assistant, tool messages)
    pub conversation_history: Vec<ChatCompletionRequestMessage>,
    /// Accumulated response text from current streaming request
    pub current_response: String,
    /// History of command suggestions in this session
    pub command_suggestions: Vec<CommandSuggestionRecord>,
    /// Index of the current pending suggestion (if any)
    pub pending_suggestion_idx: Option<usize>,
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
            current_response: String::new(),
            command_suggestions: Vec::new(),
            pending_suggestion_idx: None,
        })
    }
}

/// Manages multiple AI sessions and handles communication with OpenAI.
///
/// This is the single source of truth for conversation data. It owns both
/// the sender (for spawned API tasks) and receiver (for processing responses)
/// of the AI stream channel.
pub struct AiSessionManager {
    sessions: HashMap<SessionId, AiSession>,
    current_id: SessionId,
    next_id: SessionId,
    /// Sender for spawned async tasks to send streaming data
    ai_stream_tx: Sender<AiStreamData>,
    /// Receiver for processing streaming data from API tasks
    ai_stream_rx: Receiver<AiStreamData>,
    app_event_tx: UnboundedSender<AppEvent>,
    client: Client<async_openai::config::OpenAIConfig>,
    model: String,
}

impl AiSessionManager {
    /// Channel buffer size for AI streaming data
    const STREAM_CHANNEL_BUFFER: usize = 256;

    pub fn new(
        app_event_tx: UnboundedSender<AppEvent>,
        model: impl Into<String>,
    ) -> Result<Self, OpenAIError> {
        // Create the AI stream channel (owned entirely by this manager)
        let (ai_stream_tx, ai_stream_rx) =
            tokio::sync::mpsc::channel(Self::STREAM_CHANNEL_BUFFER);

        let system_prompt = prompt::SYSTEM_PROMPT.to_string();
        let mut manager = Self {
            sessions: HashMap::new(),
            current_id: 1,
            next_id: 2,
            ai_stream_tx,
            ai_stream_rx,
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

    /// Get the pending command suggestion for a session, if any
    pub fn get_pending_suggestion(&self, session_id: SessionId) -> Option<&CommandSuggestionRecord> {
        let session = self.sessions.get(&session_id)?;
        let idx = session.pending_suggestion_idx?;
        session.command_suggestions.get(idx)
    }

    /// Execute the suggested command for a session.
    /// This sends an ExecuteAiCommand event to the app layer.
    pub fn execute_suggestion(&self, session_id: SessionId, command: String) -> anyhow::Result<()> {
        self.app_event_tx
            .send(AppEvent::ExecuteAiCommand { session_id, command })?;
        Ok(())
    }

    /// Accept the pending command suggestion.
    ///
    /// Updates the suggestion status to Accepted, adds a Tool message to history,
    /// and returns the command string. Returns None if there's no pending suggestion.
    pub fn accept_suggestion(&mut self, session_id: SessionId) -> Option<String> {
        let session = self.sessions.get_mut(&session_id)?;
        let idx = session.pending_suggestion_idx.take()?;
        let record = session.command_suggestions.get_mut(idx)?;
        record.status = CommandSuggestionStatus::Accepted;
        let command = record.command.clone();
        let tool_call_id = record.tool_call_id.clone();

        // Add Tool message to conversation history
        // This tells the AI that the user accepted and executed the command
        if let Ok(tool_msg) = ChatCompletionRequestToolMessageArgs::default()
            .tool_call_id(tool_call_id)
            .content(format!(
                "User accepted and executed the command: {}",
                command
            ))
            .build()
        {
            session.conversation_history.push(tool_msg.into());
        }

        Some(command)
    }

    /// Reject the pending command suggestion.
    ///
    /// Updates the suggestion status to Rejected and adds a Tool message to history.
    pub fn reject_suggestion(&mut self, session_id: SessionId) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            if let Some(idx) = session.pending_suggestion_idx.take() {
                if let Some(record) = session.command_suggestions.get_mut(idx) {
                    record.status = CommandSuggestionStatus::Rejected;
                    let tool_call_id = record.tool_call_id.clone();

                    // Add Tool message to conversation history
                    // This tells the AI that the user rejected the command
                    if let Ok(tool_msg) = ChatCompletionRequestToolMessageArgs::default()
                        .tool_call_id(tool_call_id)
                        .content("User rejected this command suggestion.")
                        .build()
                    {
                        session.conversation_history.push(tool_msg.into());
                    }
                }
            }
        }
    }

    /// Check if there's a pending command suggestion for a session
    pub fn has_pending_suggestion(&self, session_id: SessionId) -> bool {
        self.sessions
            .get(&session_id)
            .map(|s| s.pending_suggestion_idx.is_some())
            .unwrap_or(false)
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
    /// 2. Sends the request to OpenAI with Tool Calling enabled
    /// 3. Streams chunks back through the ai_stream_tx channel
    /// 4. Tool calls are accumulated and sent as AiStreamData::ToolCall
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
                if let Err(e) = self.ai_stream_tx.try_send(AiStreamData::Error {
                    session_id,
                    error: "Session not found".to_string(),
                }) {
                    error!("Failed to send error event: {:?}", e);
                }
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
                if let Err(e) = self.ai_stream_tx.try_send(AiStreamData::Error {
                    session_id,
                    error: format!("Failed to build message: {}", e),
                }) {
                    error!("Failed to send error event: {:?}", e);
                }
                return;
            }
        };

        session.conversation_history.push(user_msg);
        session.current_response.clear();
        Self::trim_history(session);

        // Build OpenAI request with tools
        let request = match CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(session.conversation_history.clone())
            .tools(vec![create_suggest_command_tool()])
            .build()
        {
            Ok(req) => req,
            Err(e) => {
                if let Err(e) = self.ai_stream_tx.try_send(AiStreamData::Error {
                    session_id,
                    error: format!("Failed to build request: {}", e),
                }) {
                    error!("Failed to send error event: {:?}", e);
                }
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
                    // Accumulate tool calls during streaming
                    // Tool calls come in chunks that need to be assembled
                    let mut tool_call_map: HashMap<u32, (String, String, String)> = HashMap::new();

                    // Process streaming chunks
                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(response) => {
                                for choice in response.choices {
                                    // Handle text content
                                    if let Some(content) = choice.delta.content {
                                        if let Err(e) = stream_tx
                                            .send(AiStreamData::Chunk {
                                                session_id,
                                                text: content,
                                            })
                                            .await
                                        {
                                            error!("Failed to send chunk event: {:?}", e);
                                        }
                                    }

                                    // Handle tool calls (accumulated from chunks)
                                    if let Some(tool_calls) = choice.delta.tool_calls {
                                        for tc_chunk in tool_calls {
                                            let entry = tool_call_map
                                                .entry(tc_chunk.index)
                                                .or_insert_with(|| {
                                                    (String::new(), String::new(), String::new())
                                                });

                                            // Accumulate ID
                                            if let Some(id) = tc_chunk.id {
                                                entry.0 = id;
                                            }

                                            // Accumulate function name and arguments
                                            if let Some(func) = tc_chunk.function {
                                                if let Some(name) = func.name {
                                                    entry.1 = name;
                                                }
                                                if let Some(args) = func.arguments {
                                                    entry.2.push_str(&args);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                if let Err(e) = stream_tx
                                    .send(AiStreamData::Error {
                                        session_id,
                                        error: format!("Stream error: {}", e),
                                    })
                                    .await
                                {
                                    error!("Failed to send error event: {:?}", e);
                                }
                                return;
                            }
                        }
                    }

                    // Stream completed - send tool calls if any
                    if !tool_call_map.is_empty() {
                        // Convert accumulated chunks to tool calls
                        let tool_calls: Vec<_> = tool_call_map
                            .into_iter()
                            .map(|(_, (id, name, args))| (id, name, args))
                            .collect();

                        if let Err(e) = stream_tx
                            .send(AiStreamData::ToolCalls {
                                session_id,
                                tool_calls,
                            })
                            .await
                        {
                            error!("Failed to send tool calls event: {:?}", e);
                        }
                    }

                    // Signal end of stream
                    if let Err(e) = stream_tx.send(AiStreamData::End { session_id }).await {
                        error!("Failed to send end event: {:?}", e);
                    }
                }
                Err(e) => {
                    if let Err(e) = stream_tx
                        .send(AiStreamData::Error {
                            session_id,
                            error: format!("API error: {}", e),
                        })
                        .await
                    {
                        error!("Failed to send error event: {:?}", e);
                    }
                }
            }
        });
    }

    /// Process tool calls received from the AI.
    /// Stores the assistant message with tool calls and extracts command suggestions.
    fn process_tool_calls(
        &mut self,
        session_id: SessionId,
        tool_calls: Vec<(String, String, String)>,
    ) -> Option<(String, String)> {
        let session = self.sessions.get_mut(&session_id)?;

        // Get any accumulated text response
        let text_response = std::mem::take(&mut session.current_response);

        // Build tool call objects for the assistant message
        let tc_objects: Vec<ChatCompletionMessageToolCall> = tool_calls
            .iter()
            .map(|(id, name, args)| ChatCompletionMessageToolCall {
                id: id.clone(),
                r#type: ChatCompletionToolType::Function,
                function: async_openai::types::FunctionCall {
                    name: name.clone(),
                    arguments: args.clone(),
                },
            })
            .collect();

        // Add assistant message to history (with tool_calls)
        let mut assistant_builder = ChatCompletionRequestAssistantMessageArgs::default();
        if !text_response.is_empty() {
            assistant_builder.content(text_response);
        }
        assistant_builder.tool_calls(tc_objects.clone());

        if let Ok(assistant_msg) = assistant_builder.build() {
            session.conversation_history.push(assistant_msg.into());
        }

        Self::trim_history(session);

        // Process suggest_command tool calls
        for (id, name, args) in tool_calls {
            if name == TOOL_SUGGEST_COMMAND {
                // Parse the JSON arguments
                if let Ok(suggestion) = serde_json::from_str::<SuggestCommandArgs>(&args) {
                    let record = CommandSuggestionRecord {
                        tool_call_id: id,
                        command: suggestion.command.clone(),
                        explanation: suggestion.explanation.clone(),
                        status: CommandSuggestionStatus::Pending,
                    };
                    session.command_suggestions.push(record);
                    session.pending_suggestion_idx = Some(session.command_suggestions.len() - 1);

                    return Some((suggestion.command, suggestion.explanation));
                }
            }
        }

        None
    }

    /// Finalize a text-only response (no tool calls).
    /// This is called when the stream ends without tool calls.
    fn finalize_text_response(&mut self, session_id: SessionId) {
        let session = match self.sessions.get_mut(&session_id) {
            Some(s) => s,
            None => return,
        };

        // Get the accumulated response
        let response = std::mem::take(&mut session.current_response);

        if response.is_empty() {
            return;
        }

        // Add assistant message to conversation history
        if let Ok(assistant_msg) = ChatCompletionRequestAssistantMessageArgs::default()
            .content(response)
            .build()
        {
            session.conversation_history.push(assistant_msg.into());
        }

        Self::trim_history(session);
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

    // =========================================================================
    // Stream Processing
    // =========================================================================

    /// Receive and process AI stream data.
    ///
    /// This method:
    /// 1. Awaits data from the AI stream channel
    /// 2. Stores the data in the appropriate session
    /// 3. Returns an AiUiUpdate for the App to forward to TuiAssistant
    ///
    /// Call this in a tokio::select! branch in the main event loop.
    pub async fn recv_ai_stream(&mut self) -> Option<AiUiUpdate> {
        let data = self.ai_stream_rx.recv().await?;

        match data {
            AiStreamData::Chunk { session_id, text } => {
                // Store chunk in session
                self.append_chunk(session_id, &text);
                // Return update for UI
                Some(AiUiUpdate::Chunk { session_id, text })
            }

            AiStreamData::ToolCalls {
                session_id,
                tool_calls,
            } => {
                // Process tool calls and extract command suggestion
                if let Some((command, explanation)) =
                    self.process_tool_calls(session_id, tool_calls)
                {
                    Some(AiUiUpdate::CommandSuggestion {
                        session_id,
                        command,
                        explanation,
                    })
                } else {
                    // Tool calls processed but no command suggestion
                    None
                }
            }

            AiStreamData::End { session_id } => {
                // Finalize any text-only response
                self.finalize_text_response(session_id);
                Some(AiUiUpdate::End { session_id })
            }

            AiStreamData::Error { session_id, error } => {
                Some(AiUiUpdate::Error { session_id, error })
            }
        }
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
