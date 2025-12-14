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
use crate::utils::shell2::collect_shell2_system_context;

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
    /// User chose another command (auto-rejected)
    Ignored,
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
    /// User messages are JSON-formatted and can be parsed to extract the original request.
    pub conversation_history: Vec<ChatCompletionRequestMessage>,
    /// Accumulated response text from current streaming request
    pub current_response: String,
    /// History of command suggestions in this session
    pub command_suggestions: Vec<CommandSuggestionRecord>,
    /// Indices of pending suggestions from the most recent AI response (supports multiple tool calls)
    pub pending_suggestion_indices: Vec<usize>,
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
            pending_suggestion_indices: Vec::new(),
        })
    }

    /// Clear conversation history, keeping only the system prompt.
    fn clear(&mut self) {
        // Keep only the first message (system prompt)
        self.conversation_history.truncate(1);
        self.current_response.clear();
        self.command_suggestions.clear();
        self.pending_suggestion_indices.clear();
    }

    /// Convert conversation history to UI-displayable ChatMessage format.
    ///
    /// This parses user messages from JSON format to extract the original request,
    /// and includes assistant messages and command cards.
    pub fn to_ui_messages(&self) -> Vec<crate::ui::assistant::ChatMessage> {
        use crate::ui::assistant::{ChatMessage, CommandStatus};

        let mut messages = Vec::new();
        let mut command_idx = 0;

        for msg in &self.conversation_history {
            match msg {
                ChatCompletionRequestMessage::User(user_msg) => {
                    // Extract text content from user message
                    let prompt_text = match &user_msg.content {
                        async_openai::types::ChatCompletionRequestUserMessageContent::Text(t) => t,
                        async_openai::types::ChatCompletionRequestUserMessageContent::Array(_) => {
                            continue;
                        }
                    };

                    // Parse JSON to extract original user request
                    let user_request = prompt::extract_user_request(prompt_text)
                        .unwrap_or_else(|| prompt_text.clone());

                    messages.push(ChatMessage::User { text: user_request });
                }
                ChatCompletionRequestMessage::Assistant(asst_msg) => {
                    // Extract text content from assistant message (may be empty for tool-call-only responses)
                    let text_content = asst_msg.content.as_ref().and_then(|content| {
                        match content {
                            async_openai::types::ChatCompletionRequestAssistantMessageContent::Text(t) => {
                                if t.is_empty() { None } else { Some(t.clone()) }
                            }
                            async_openai::types::ChatCompletionRequestAssistantMessageContent::Array(_) => None,
                        }
                    });

                    // Check if this assistant message has tool calls
                    let has_tool_calls = asst_msg.tool_calls.as_ref().map(|tc| !tc.is_empty()).unwrap_or(false);

                    // Add assistant text message if present, or empty placeholder if only tool calls
                    if let Some(text) = text_content {
                        messages.push(ChatMessage::Assistant {
                            text,
                            is_streaming: false,
                        });
                    } else if has_tool_calls {
                        // Add empty assistant message to match real-time behavior
                        messages.push(ChatMessage::Assistant {
                            text: String::new(),
                            is_streaming: false,
                        });
                    }

                    // Add command cards for tool calls
                    if let Some(tool_calls) = &asst_msg.tool_calls {
                        for _ in tool_calls {
                            if let Some(record) = self.command_suggestions.get(command_idx) {
                                let status = match record.status {
                                    CommandSuggestionStatus::Pending => CommandStatus::Pending,
                                    CommandSuggestionStatus::Accepted => CommandStatus::Executed,
                                    CommandSuggestionStatus::Rejected | CommandSuggestionStatus::Ignored => CommandStatus::Rejected,
                                };
                                // Evaluate command security (verdict now contains reason)
                                let verdict = crate::security::evaluate(&record.command);
                                messages.push(ChatMessage::CommandCard {
                                    command: record.command.clone(),
                                    explanation: record.explanation.clone(),
                                    status,
                                    verdict,
                                });
                                command_idx += 1;
                            }
                        }
                    }
                }
                // System and Tool messages are not displayed to the user
                _ => {}
            }
        }

        messages
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

    /// Get all session tabs for UI rendering.
    ///
    /// Returns a list of SessionTab structs sorted by session ID.
    pub fn get_session_tabs(&self) -> Vec<crate::ui::assistant::SessionTab> {
        use crate::ui::assistant::SessionTab;

        let mut tabs: Vec<_> = self.sessions.keys()
            .map(|&id| SessionTab {
                id,
                name: format!("Session {}", id),
            })
            .collect();
        tabs.sort_by_key(|t| t.id);
        tabs
    }

    /// Get the messages for a specific session in UI format.
    ///
    /// This parses user messages from JSON to extract the original request,
    /// and includes assistant responses and command cards.
    /// If there's an in-progress streaming response, it's included as well.
    pub fn get_session_messages(&self, session_id: SessionId) -> Vec<crate::ui::assistant::ChatMessage> {
        use crate::ui::assistant::ChatMessage;

        let Some(session) = self.sessions.get(&session_id) else {
            return Vec::new();
        };

        let mut messages = session.to_ui_messages();

        // If there's an in-progress streaming response, add it
        if !session.current_response.is_empty() {
            messages.push(ChatMessage::Assistant {
                text: session.current_response.clone(),
                is_streaming: true,
            });
        }

        messages
    }

    /// Get the next session ID (cycles through sessions).
    ///
    /// Returns the session ID after the current one, wrapping to the first if at the end.
    pub fn next_session_id(&self) -> Option<SessionId> {
        if self.sessions.is_empty() {
            return None;
        }
        let mut ids: Vec<_> = self.sessions.keys().copied().collect();
        ids.sort();
        let current_idx = ids.iter().position(|&id| id == self.current_id).unwrap_or(0);
        let next_idx = (current_idx + 1) % ids.len();
        Some(ids[next_idx])
    }

    /// Get the previous session ID (cycles through sessions).
    ///
    /// Returns the session ID before the current one, wrapping to the last if at the beginning.
    pub fn prev_session_id(&self) -> Option<SessionId> {
        if self.sessions.is_empty() {
            return None;
        }
        let mut ids: Vec<_> = self.sessions.keys().copied().collect();
        ids.sort();
        let current_idx = ids.iter().position(|&id| id == self.current_id).unwrap_or(0);
        let prev_idx = if current_idx == 0 {
            ids.len() - 1
        } else {
            current_idx - 1
        };
        Some(ids[prev_idx])
    }

    pub fn switch_session(&mut self, session_id: SessionId) -> bool {
        if self.sessions.contains_key(&session_id) {
            self.current_id = session_id;
            true
        } else {
            false
        }
    }

    /// Get all pending command suggestions for a session.
    /// Returns a vector of (command, explanation) tuples.
    pub fn get_pending_suggestions(&self, session_id: SessionId) -> Vec<(String, String)> {
        let Some(session) = self.sessions.get(&session_id) else {
            return Vec::new();
        };

        session.pending_suggestion_indices
            .iter()
            .filter_map(|&idx| session.command_suggestions.get(idx))
            .map(|r| (r.command.clone(), r.explanation.clone()))
            .collect()
    }

    /// Get a specific pending suggestion by its position in the pending list.
    pub fn get_pending_suggestion_at(&self, session_id: SessionId, pending_idx: usize) -> Option<&CommandSuggestionRecord> {
        let session = self.sessions.get(&session_id)?;
        let &actual_idx = session.pending_suggestion_indices.get(pending_idx)?;
        session.command_suggestions.get(actual_idx)
    }

    /// Execute the suggested command for a session.
    /// This sends an ExecuteAiCommand event to the app layer.
    pub fn execute_suggestion(&self, session_id: SessionId, command: String) -> anyhow::Result<()> {
        self.app_event_tx
            .send(AppEvent::ExecuteAiCommand { session_id, command })?;
        Ok(())
    }

    /// Accept the pending command suggestion at the given index.
    ///
    /// Updates the suggestion status to Accepted, marks other pending suggestions as Ignored,
    /// and returns the command string. Returns None if the index is invalid.
    ///
    /// Note: Tool messages are NOT added here. They are added later by
    /// `respond_all_pending_tool_calls` before sending the next message.
    pub fn accept_suggestion(&mut self, session_id: SessionId, pending_idx: usize) -> Option<String> {
        let session = self.sessions.get_mut(&session_id)?;

        // Get the actual index in command_suggestions
        let &actual_idx = session.pending_suggestion_indices.get(pending_idx)?;

        // Mark the selected command as Accepted
        let record = session.command_suggestions.get_mut(actual_idx)?;
        record.status = CommandSuggestionStatus::Accepted;
        let command = record.command.clone();

        // Mark all other pending suggestions as Ignored
        for (i, &idx) in session.pending_suggestion_indices.iter().enumerate() {
            if i != pending_idx {
                if let Some(other_record) = session.command_suggestions.get_mut(idx) {
                    other_record.status = CommandSuggestionStatus::Ignored;
                }
            }
        }

        // Clear pending indices (all have been processed)
        session.pending_suggestion_indices.clear();

        Some(command)
    }

    /// Reject all pending command suggestions.
    ///
    /// Updates all pending suggestion statuses to Rejected.
    /// Note: Tool messages are NOT added here. They are added later by
    /// `respond_all_pending_tool_calls` before sending the next message.
    pub fn reject_suggestion(&mut self, session_id: SessionId) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            // Mark all pending suggestions as Rejected
            for &idx in &session.pending_suggestion_indices {
                if let Some(record) = session.command_suggestions.get_mut(idx) {
                    record.status = CommandSuggestionStatus::Rejected;
                }
            }

            // Clear pending indices
            session.pending_suggestion_indices.clear();
        }
    }

    /// Add tool response messages for all command suggestions that haven't been responded to yet.
    ///
    /// This must be called before sending a new message to ensure the conversation history
    /// is valid (every tool_call must have a corresponding tool message).
    ///
    /// Iterates through all command suggestions and adds a tool message for any that
    /// have a non-Pending status but haven't been responded to yet.
    fn respond_all_pending_tool_calls(&mut self, session_id: SessionId) {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return;
        };

        // Find tool calls that need responses by looking at the last assistant message
        let Some(last_assistant_idx) = session.conversation_history.iter().rposition(|msg| {
            matches!(msg, ChatCompletionRequestMessage::Assistant(_))
        }) else {
            return;
        };

        // Get the tool call IDs from the last assistant message
        let tool_call_ids: Vec<String> = if let ChatCompletionRequestMessage::Assistant(asst_msg) =
            &session.conversation_history[last_assistant_idx]
        {
            asst_msg
                .tool_calls
                .as_ref()
                .map(|tcs| tcs.iter().map(|tc| tc.id.clone()).collect())
                .unwrap_or_default()
        } else {
            return;
        };

        if tool_call_ids.is_empty() {
            return;
        }

        // Check which tool calls already have responses
        let existing_responses: std::collections::HashSet<String> = session
            .conversation_history
            .iter()
            .skip(last_assistant_idx + 1)
            .filter_map(|msg| {
                if let ChatCompletionRequestMessage::Tool(tool_msg) = msg {
                    Some(tool_msg.tool_call_id.clone())
                } else {
                    None
                }
            })
            .collect();

        // Find command suggestions that match the tool call IDs and need responses
        for tool_call_id in tool_call_ids {
            if existing_responses.contains(&tool_call_id) {
                continue; // Already has a response
            }

            // Find the corresponding command suggestion
            let suggestion = session
                .command_suggestions
                .iter()
                .find(|r| r.tool_call_id == tool_call_id);

            let response_content = if let Some(record) = suggestion {
                match record.status {
                    CommandSuggestionStatus::Pending => {
                        // Still pending - user hasn't decided yet, mark as ignored
                        "User did not respond to this suggestion."
                    }
                    CommandSuggestionStatus::Accepted => {
                        // This should have been responded to already, but add it anyway
                        "User accepted and executed this command."
                    }
                    CommandSuggestionStatus::Rejected => "User rejected this command suggestion.",
                    CommandSuggestionStatus::Ignored => {
                        "User chose a different command from the suggestions."
                    }
                }
            } else {
                // Unknown tool call (shouldn't happen, but handle gracefully)
                "Tool call acknowledged."
            };

            // Add tool message
            if let Ok(tool_msg) = ChatCompletionRequestToolMessageArgs::default()
                .tool_call_id(tool_call_id)
                .content(response_content)
                .build()
            {
                session.conversation_history.push(tool_msg.into());
            }
        }

        // Clear any remaining pending indices (they've all been handled now)
        session.pending_suggestion_indices.clear();
    }

    /// Check if there's any pending command suggestion for a session
    pub fn has_pending_suggestion(&self, session_id: SessionId) -> bool {
        self.sessions
            .get(&session_id)
            .map(|s| !s.pending_suggestion_indices.is_empty())
            .unwrap_or(false)
    }

    /// Get the number of pending suggestions for a session
    pub fn pending_suggestion_count(&self, session_id: SessionId) -> usize {
        self.sessions
            .get(&session_id)
            .map(|s| s.pending_suggestion_indices.len())
            .unwrap_or(0)
    }

    pub fn new_session(&mut self) -> Result<SessionId, OpenAIError> {
        let id = self.next_id;
        self.next_id += 1;
        let system_prompt = prompt::SYSTEM_PROMPT.to_string();
        self.sessions.insert(id, AiSession::new(id, system_prompt)?);
        self.current_id = id;
        Ok(id)
    }

    /// Close a session and switch to an adjacent one.
    ///
    /// Returns the new active session ID, or None if this was the last session
    /// (in which case the session is not closed).
    pub fn close_session(&mut self, session_id: SessionId) -> Option<SessionId> {
        // If this is the last session, clear it instead of closing
        if self.sessions.len() <= 1 {
            if let Some(session) = self.sessions.get_mut(&session_id) {
                session.clear();
            }
            return Some(session_id);
        }

        // Find the next session to switch to before removing
        let mut ids: Vec<_> = self.sessions.keys().copied().collect();
        ids.sort();
        let current_idx = ids.iter().position(|&id| id == session_id)?;

        // Choose next session (prefer next, fallback to previous)
        let new_id = if current_idx + 1 < ids.len() {
            ids[current_idx + 1]
        } else {
            ids[current_idx.saturating_sub(1)]
        };

        // Remove the session
        self.sessions.remove(&session_id);

        // Switch to the new session
        self.current_id = new_id;
        Some(new_id)
    }

    /// Send a message to the AI with system context and receive a streaming response.
    ///
    /// This method:
    /// 1. Responds to any pending tool calls (ensures valid history)
    /// 2. Builds a prompt with user query + system context (cwd, env, history)
    /// 3. Sends the request to OpenAI with Tool Calling enabled
    /// 4. Streams chunks back through the ai_stream_tx channel
    /// 5. Tool calls are accumulated and sent as AiStreamData::ToolCall
    pub fn send_message(
        &mut self,
        session_id: SessionId,
        user_input: &str,
        context: ContextSnapshot,
    ) {
        // First, ensure all previous tool calls have responses
        self.respond_all_pending_tool_calls(session_id);

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

        // Build prompt with context (JSON format for reliable extraction)
        let prompt = match prompt::build_prompt(user_input, &context) {
            Ok(p) => p,
            Err(e) => {
                if let Err(e) = self.ai_stream_tx.try_send(AiStreamData::Error {
                    session_id,
                    error: format!("Failed to build prompt: {}", e),
                }) {
                    error!("Failed to send error event: {:?}", e);
                }
                return;
            }
        };

        // Create user message for OpenAI API
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
        let base_messages = session.conversation_history.clone();
        let model = self.model.clone();

        // Log the request JSON
        if let Ok(request_json) = serde_json::to_string_pretty(&request) {
            tracing::info!("Sending request to OpenAI API (session {}): {}", session_id, request_json);
        }

        // Clone what we need for the async task
        let stream_tx = self.ai_stream_tx.clone();
        let client = self.client.clone();
        let tool = create_suggest_command_tool();

        // Spawn async task to handle streaming
        tokio::spawn(async move {
            // Shell2: collect extra system context (best-effort) before the network call.
            let shell2_ctx = collect_shell2_system_context(&context).await;

            // Build the OpenAI request messages:
            // - include the persisted conversation history
            // - inject Shell2 context as an additional system message (request-only)
            let mut messages = base_messages;
            if !shell2_ctx.is_empty() {
                if let Ok(sys_msg) = ChatCompletionRequestSystemMessageArgs::default()
                    .content(format!("Shell2 system context (read-only):\n{}", shell2_ctx))
                    .build()
                {
                    // Insert right after the primary system prompt when possible.
                    let idx = if messages.is_empty() { 0 } else { 1.min(messages.len()) };
                    messages.insert(idx, sys_msg.into());
                }
            }

            let request = match CreateChatCompletionRequestArgs::default()
                .model(&model)
                .messages(messages)
                .tools(vec![tool])
                .build()
            {
                Ok(req) => req,
                Err(e) => {
                    if let Err(e) = stream_tx
                        .send(AiStreamData::Error {
                            session_id,
                            error: format!("Failed to build request: {}", e),
                        })
                        .await
                    {
                        error!("Failed to send error event: {:?}", e);
                    }
                    return;
                }
            };

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
    /// Stores the assistant message with tool calls and extracts ALL command suggestions.
    /// Returns a vector of (command, explanation) tuples for UI display.
    fn process_tool_calls(
        &mut self,
        session_id: SessionId,
        tool_calls: Vec<(String, String, String)>,
    ) -> Vec<(String, String)> {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Vec::new();
        };

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

        // Clear any previous pending indices (new batch of tool calls)
        session.pending_suggestion_indices.clear();

        // Process ALL suggest_command tool calls
        let mut commands = Vec::new();
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
                    // Track this as a pending suggestion
                    session.pending_suggestion_indices.push(session.command_suggestions.len() - 1);

                    commands.push((suggestion.command, suggestion.explanation));
                }
            }
        }

        commands
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
                // Process all tool calls and extract command suggestions
                let commands = self.process_tool_calls(session_id, tool_calls);
                if commands.is_empty() {
                    // Tool calls processed but no command suggestions
                    None
                } else {
                    Some(AiUiUpdate::CommandSuggestion {
                        session_id,
                        commands,
                    })
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
