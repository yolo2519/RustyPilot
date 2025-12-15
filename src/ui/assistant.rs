//! TUI Assistant sidebar component for AI chat interactions.
//!
//! This module provides a sidebar widget that supports:
//! - Multiple chat sessions with tab switching
//! - Chat message display (user messages, AI responses, command cards)
//! - Streaming AI response rendering
//! - Command suggestion cards with execute/cancel actions
//! - Multi-line text input with cursor support (Ctrl+O for newline)

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::Buffer;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use unicode_width::UnicodeWidthStr;
use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::ai::session::SessionId;
use crate::event::AiUiUpdate;
use crate::security::Verdict;
use super::visual::{VisualState, SelectionMode, PaneStatus, KeyHandleResult, copy_to_clipboard, is_in_selection_with_mode};

// ============================================================================
// Data Structures
// ============================================================================

/// Status of a command suggestion card
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    /// Waiting for user confirmation (Ctrl+Y/Ctrl+N)
    Pending,
    /// User confirmed and command was sent to shell
    Executed,
    /// User rejected the command
    Rejected,
}

/// A chat message in the conversation
#[derive(Debug, Clone)]
pub enum ChatMessage {
    /// Message from the user
    User { text: String },
    /// Response from the AI assistant
    Assistant {
        text: String,
        /// Whether the message is still being streamed
        is_streaming: bool,
    },
    /// A command suggestion card
    CommandCard {
        command: String,
        explanation: String,
        status: CommandStatus,
        verdict: Verdict,
    },
    /// An error message from the system
    Error { text: String },
}

/// A session tab displayed in the tab bar
#[derive(Debug, Clone)]
pub struct SessionTab {
    pub id: SessionId,
    pub name: String,
}

/// Result of clicking on the tab bar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabClickResult {
    /// Switch to a different session
    SwitchToTab(SessionId),
    /// Create a new session ("+" button clicked)
    NewTab,
    /// Close the specified session ("x" button on active tab)
    CloseTab(SessionId),
    /// No action (clicked on empty area)
    None,
}

/// Result of clicking on the message area
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageAreaClickResult {
    /// Next command button clicked (for pagination)
    NextCommand(usize),
    /// Execute command button clicked on a pending command card
    ExecuteCommand(usize),
    /// Cancel command button clicked on a pending command card
    CancelCommand(usize),
    /// No special action (normal area click)
    None,
}

// ============================================================================
// TuiAssistant Widget
// ============================================================================

/// The main AI Assistant sidebar widget.
///
/// This is a pure UI component that receives updates from AiSessionManager
/// through the App layer. It does not own any data channels.
///
/// Session state is synchronized from the backend (AiSessionManager).
/// The frontend only maintains UI state and caches for rendering.
pub struct TuiAssistant {
    // Session management (synced from backend)
    session_tabs: Vec<SessionTab>,
    active_session: SessionId,

    // Current session's messages (UI rendering format, cached from backend)
    messages: Vec<ChatMessage>,

    // Input state
    input_buffer: String,
    input_cursor: usize,

    // Input selection state (byte offset of selection anchor, None = no selection)
    input_selection_anchor: Option<usize>,

    // Scroll state (0 = at bottom, >0 = scrolled up by N lines)
    scroll_offset: usize,

    // Pending command (index into messages vec for the first command card)
    pending_command_idx: Option<usize>,

    // Multi-command suggestion state
    /// All pending commands from the AI (command, explanation, verdict)
    pending_commands: Vec<(String, String, Verdict)>,
    /// Currently displayed suggestion index (0-based, for cycling through suggestions)
    current_suggestion_idx: usize,

    // Cached rendering dimensions (updated during render, uses Cell for interior mutability)
    last_input_area_width: Cell<u16>,

    // Cached max scroll offset (updated during render)
    max_scroll_offset: Cell<usize>,

    // Visual mode state
    visual_state: Option<VisualState>,

    // Cached total lines for visual mode (updated during render)
    cached_total_lines: Cell<usize>,

    // Cached visible width for visual mode
    cached_visible_width: Cell<usize>,

    // Cached tab positions for mouse click detection [(start_x, end_x, tab_id, is_close_button)]
    // Updated during render_tab_bar
    cached_tab_positions: std::cell::RefCell<Vec<TabHitArea>>,

    // Cached command card positions for mouse click detection
    // Updated during render_message_list
    cached_command_cards: std::cell::RefCell<Vec<CommandCardHitArea>>,
}

/// Hit area for a tab in the tab bar
#[derive(Debug, Clone, Copy)]
pub struct TabHitArea {
    /// Start X position (screen coordinates)
    pub start_x: u16,
    /// End X position (exclusive)
    pub end_x: u16,
    /// Session ID (None for "+" button)
    pub session_id: Option<SessionId>,
    /// Whether this is the close button area
    pub is_close_button: bool,
}

/// Hit area for a command card in the message area
#[derive(Debug, Clone, Copy)]
pub struct CommandCardHitArea {
    /// Message index in the messages array
    pub message_idx: usize,
    /// Start Y position (screen coordinates, relative to message area)
    pub start_y: u16,
    /// End Y position (exclusive)
    pub end_y: u16,
    /// Button row Y position (relative to message area)
    pub button_y: Option<u16>,
    /// Next button area (start_x, end_x) - only present with pagination
    pub next_btn: Option<(u16, u16)>,
    /// Execute button area (start_x, end_x)
    pub execute_btn: Option<(u16, u16)>,
    /// Cancel button area (start_x, end_x)
    pub cancel_btn: Option<(u16, u16)>,
}

impl TuiAssistant {
    /// Input prompt prefix (normal state)
    const INPUT_PROMPT: &'static str = "> ";
    /// Input prompt when AI is streaming (same length as normal prompt)
    const STREAMING_PROMPT: &'static str = "⋯ ";

    /// Get the input prompt based on current state
    fn prompt(&self) -> &str {
        if self.is_streaming() {
            Self::STREAMING_PROMPT
        } else {
            Self::INPUT_PROMPT
        }
    }

    /// Get the input prompt width in characters
    fn prompt_width(&self) -> u16 {
        self.prompt().width() as u16
    }
}

impl TuiAssistant {
    pub fn new() -> Self {
        // Initial session tab - will be overwritten by sync_session_tabs()
        let initial_session = SessionTab {
            id: 1,
            name: "Session 1".to_string(),
        };
        Self {
            session_tabs: vec![initial_session],
            active_session: 1,
            messages: Vec::new(),
            input_buffer: String::new(),
            input_cursor: 0,
            input_selection_anchor: None,
            scroll_offset: 0,
            pending_command_idx: None,
            pending_commands: Vec::new(),
            current_suggestion_idx: 0,
            last_input_area_width: Cell::new(80), // Default value
            max_scroll_offset: Cell::new(0),
            visual_state: None,
            cached_total_lines: Cell::new(0),
            cached_visible_width: Cell::new(80),
            cached_tab_positions: std::cell::RefCell::new(Vec::new()),
            cached_command_cards: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Get the last known input area width (updated during rendering)
    pub fn input_area_width(&self) -> u16 {
        self.last_input_area_width.get()
    }

    /// Handle AI updates forwarded from AiSessionManager via App.
    ///
    /// This is the main entry point for AI data updates. The App layer
    /// receives updates from AiSessionManager and forwards them here.
    pub fn handle_ai_update(&mut self, update: AiUiUpdate) {
        match update {
            AiUiUpdate::Chunk { session_id, text } => {
                if session_id == self.active_session {
                    self.append_stream_chunk(&text);
                }
            }
            AiUiUpdate::End { session_id } => {
                if session_id == self.active_session {
                    self.end_stream();
                }
            }
            AiUiUpdate::Error { session_id, error } => {
                if session_id == self.active_session {
                    self.end_stream();
                    self.push_error_message(error);
                }
            }
            AiUiUpdate::CommandSuggestion {
                session_id,
                commands,
            } => {
                if session_id == self.active_session {
                    // End the streaming message first
                    self.end_stream();
                    // Store all commands and show the first one
                    self.set_pending_commands(commands);
                }
            }
        }
    }
}

impl Default for TuiAssistant {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiAssistant {
    // ========================================================================
    // Session Management (synced from backend)
    // ========================================================================

    /// Sync session tabs from backend.
    ///
    /// This should be called by the App layer to update the UI with the current
    /// list of sessions from AiSessionManager.
    pub fn sync_session_tabs(&mut self, tabs: Vec<SessionTab>) {
        self.session_tabs = tabs;
    }

    /// Load messages for the current session from backend.
    ///
    /// This should be called after switching sessions to populate the message list.
    pub fn load_messages(&mut self, messages: Vec<ChatMessage>) {
        self.messages = messages;
        self.scroll_offset = 0;
        // Find pending command card index if any
        self.pending_command_idx = self.messages.iter().position(|m| {
            matches!(m, ChatMessage::CommandCard { status: CommandStatus::Pending, .. })
        });
    }

    /// Switch to a different session by ID.
    ///
    /// This only updates the active session ID. The caller should also call
    /// `load_messages()` with the messages from the backend.
    pub fn switch_session(&mut self, id: SessionId) {
        if self.active_session != id {
            self.active_session = id;
            // Clear messages - they should be loaded by load_messages()
            self.messages.clear();
            self.scroll_offset = 0;
            self.pending_command_idx = None;
            // Clear multi-command state
            self.pending_commands.clear();
            self.current_suggestion_idx = 0;
        }
    }

    /// Get the current active session ID
    pub fn active_session_id(&self) -> SessionId {
        self.active_session
    }

    /// Get all session tabs
    pub fn session_tabs(&self) -> &[SessionTab] {
        &self.session_tabs
    }

    // ========================================================================
    // Message Management
    // ========================================================================

    /// Add a user message to the conversation
    pub fn push_user_message(&mut self, text: String) {
        self.messages.push(ChatMessage::User { text });
        self.scroll_to_bottom();
    }

    /// Add an error message to the conversation
    pub fn push_error_message(&mut self, text: String) {
        self.messages.push(ChatMessage::Error { text });
        self.scroll_to_bottom();
    }

    /// Start a new streaming assistant message
    pub fn start_assistant_message(&mut self) {
        self.messages.push(ChatMessage::Assistant {
            text: String::new(),
            is_streaming: true,
        });
        self.scroll_to_bottom();
    }

    /// Append a chunk to the current streaming message
    pub fn append_stream_chunk(&mut self, chunk: &str) {
        if let Some(ChatMessage::Assistant { text, is_streaming }) = self.messages.last_mut() && *is_streaming {
            text.push_str(chunk);
            self.scroll_to_bottom();
        }
    }

    /// Mark the current streaming message as complete
    pub fn end_stream(&mut self) {
        if let Some(ChatMessage::Assistant { is_streaming, .. }) = self.messages.last_mut() {
            *is_streaming = false;
        }
    }

    /// Add a command suggestion card (evaluates verdict automatically)
    pub fn push_command_card(&mut self, command: String, explanation: String) {
        let verdict = crate::security::evaluate(&command);
        self.push_command_card_with_verdict(command, explanation, verdict);
    }

    /// Add a command suggestion card with pre-evaluated verdict
    fn push_command_card_with_verdict(
        &mut self,
        command: String,
        explanation: String,
        verdict: Verdict,
    ) {
        let idx = self.messages.len();
        self.messages.push(ChatMessage::CommandCard {
            command,
            explanation,
            status: CommandStatus::Pending,
            verdict,
        });
        self.pending_command_idx = Some(idx);
        self.scroll_to_bottom();
    }

    /// Set multiple pending commands from AI response.
    /// Only displays the first command card; user can cycle through with Ctrl+A.
    /// Pre-evaluates verdict for all commands.
    pub fn set_pending_commands(&mut self, commands: Vec<(String, String)>) {
        if commands.is_empty() {
            return;
        }

        // Pre-evaluate verdict for all commands and store
        self.pending_commands = commands
            .into_iter()
            .map(|(cmd, exp)| {
                let verdict = crate::security::evaluate(&cmd);
                (cmd, exp, verdict)
            })
            .collect();
        self.current_suggestion_idx = 0;

        // Add a command card for the first command
        let (command, explanation, verdict) = self.pending_commands[0].clone();
        self.push_command_card_with_verdict(command, explanation, verdict);
    }

    /// Cycle to the next command suggestion (wraps around).
    /// Updates the displayed command card with pre-stored verdict.
    pub fn cycle_suggestion(&mut self) {
        if self.pending_commands.len() <= 1 {
            return; // Nothing to cycle
        }

        // Move to next suggestion (wrap around)
        self.current_suggestion_idx = (self.current_suggestion_idx + 1) % self.pending_commands.len();

        // Update the displayed command card with stored verdict
        if let Some(idx) = self.pending_command_idx {
            if let Some(ChatMessage::CommandCard { command, explanation, verdict, .. }) = self.messages.get_mut(idx) {
                let (new_cmd, new_exp, new_verdict) = &self.pending_commands[self.current_suggestion_idx];
                *command = new_cmd.clone();
                *explanation = new_exp.clone();
                *verdict = new_verdict.clone();
            }
        }
    }

    /// Get the current suggestion index and total count for display.
    /// Returns (current_index, total_count) where current_index is 1-based.
    pub fn suggestion_pagination(&self) -> Option<(usize, usize)> {
        if self.pending_commands.len() <= 1 {
            None // Don't show pagination for single command
        } else {
            Some((self.current_suggestion_idx + 1, self.pending_commands.len()))
        }
    }

    /// Get the currently selected suggestion index (0-based).
    /// Used by the backend to know which command the user accepted.
    pub fn current_suggestion_index(&self) -> usize {
        self.current_suggestion_idx
    }

    /// Get all messages in the current session
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    // ========================================================================
    // Command Confirmation
    // ========================================================================

    /// Check if there's a pending command waiting for confirmation
    pub fn has_pending_command(&self) -> bool {
        self.pending_command_idx.is_some()
    }

    /// Confirm the pending command (Y key) - returns the command string if allowed
    ///
    /// This method enforces verdict gating:
    /// - `Verdict::Allow`: Can be executed immediately
    /// - `Verdict::RequireConfirmation`: Can be executed after user confirmation
    /// - `Verdict::Deny`: Cannot be executed, returns None
    ///
    /// # Returns
    /// - `Some(command)` if the command can be executed (Allow or RequireConfirmation)
    /// - `None` if the command is denied or no pending command exists
    pub fn confirm_command(&mut self) -> Option<String> {
        if let Some(idx) = self.pending_command_idx.take() {
            if let Some(ChatMessage::CommandCard { command, status, verdict, .. }) =
                self.messages.get_mut(idx) {
                // Verdict gating: only Allow and RequireConfirmation can execute
                if verdict.is_allowed() {
                    *status = CommandStatus::Executed;
                    let result = command.clone();
                    // Clear multi-command state
                    self.pending_commands.clear();
                    self.current_suggestion_idx = 0;
                    return Some(result);
                } else {
                    // Deny verdict: do not execute, just clear pending state
                    *status = CommandStatus::Rejected;
                    // Clear multi-command state
                    self.pending_commands.clear();
                    self.current_suggestion_idx = 0;
                    return None;
                }
            }
        }
        None
    }

    /// Reject the pending command (N key)
    pub fn reject_command(&mut self) {
        if let Some(idx) = self.pending_command_idx.take() {
            if let Some(ChatMessage::CommandCard { status, .. }) = self.messages.get_mut(idx) {
                *status = CommandStatus::Rejected;
            }
        }
        // Clear multi-command state
        self.pending_commands.clear();
        self.current_suggestion_idx = 0;
    }

    /// Check if the current pending command has a Deny verdict
    pub fn is_pending_command_denied(&self) -> bool {
        if let Some(idx) = self.pending_command_idx {
            if let Some(ChatMessage::CommandCard { verdict, .. }) = self.messages.get(idx) {
                return verdict.is_deny();
            }
        }
        false
    }

    /// Copy the pending command to clipboard (for Deny verdict)
    /// Returns the command string if successful
    pub fn copy_pending_command(&mut self) -> Option<String> {
        if let Some(idx) = self.pending_command_idx.take() {
            if let Some(ChatMessage::CommandCard { command, status, .. }) =
                self.messages.get_mut(idx) {
                let cmd = command.clone();
                if copy_to_clipboard(&cmd) {
                    *status = CommandStatus::Executed; // "Executed" means "Copied" for Deny
                } else {
                    *status = CommandStatus::Rejected;
                }
                // Clear multi-command state
                self.pending_commands.clear();
                self.current_suggestion_idx = 0;
                return Some(cmd);
            }
        }
        None
    }

    // ========================================================================
    // Input Box
    // ========================================================================

    /// Get the current input text
    pub fn get_input(&self) -> &str {
        &self.input_buffer
    }

    /// Take the input text and clear the buffer
    pub fn take_input(&mut self) -> String {
        self.input_cursor = 0;
        std::mem::take(&mut self.input_buffer)
    }

    /// Insert a character at the cursor position
    pub fn insert_char(&mut self, c: char) {
        self.input_buffer.insert(self.input_cursor, c);
        self.input_cursor += c.len_utf8();
    }

    /// Delete the character before the cursor (backspace)
    pub fn delete_char(&mut self) {
        if self.input_cursor > 0 {
            // Find the previous character boundary
            let mut prev_pos = self.input_cursor - 1;
            while prev_pos > 0 && !self.input_buffer.is_char_boundary(prev_pos) {
                prev_pos -= 1;
            }
            self.input_buffer.remove(prev_pos);
            self.input_cursor = prev_pos;
        }
    }

    /// Delete the character at the cursor (delete key)
    pub fn delete_char_forward(&mut self) {
        if self.input_cursor < self.input_buffer.len() {
            self.input_buffer.remove(self.input_cursor);
        }
    }

    /// Move cursor left/right by delta characters
    pub fn move_cursor(&mut self, delta: i16) {
        if delta < 0 {
            // Move left
            let steps = (-delta) as usize;
            for _ in 0..steps {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    while self.input_cursor > 0
                        && !self.input_buffer.is_char_boundary(self.input_cursor)
                    {
                        self.input_cursor -= 1;
                    }
                }
            }
        } else {
            // Move right
            let steps = delta as usize;
            for _ in 0..steps {
                if self.input_cursor < self.input_buffer.len() {
                    self.input_cursor += 1;
                    while self.input_cursor < self.input_buffer.len()
                        && !self.input_buffer.is_char_boundary(self.input_cursor)
                    {
                        self.input_cursor += 1;
                    }
                }
            }
        }
    }

    /// Move cursor to the beginning of the input
    pub fn move_cursor_to_start(&mut self) {
        self.input_cursor = 0;
    }

    /// Move cursor to the end of the input
    pub fn move_cursor_to_end(&mut self) {
        self.input_cursor = self.input_buffer.len();
    }

    /// Get the current cursor position (in bytes)
    pub fn cursor_position(&self) -> usize {
        self.input_cursor
    }

    // ========================================================================
    // Input Selection
    // ========================================================================

    /// Check if there's an active selection in the input box.
    pub fn has_input_selection(&self) -> bool {
        self.input_selection_anchor.is_some()
    }

    /// Start input selection at current cursor position.
    pub fn start_input_selection(&mut self) {
        if self.input_selection_anchor.is_none() {
            self.input_selection_anchor = Some(self.input_cursor);
        }
    }

    /// Clear input selection.
    pub fn clear_input_selection(&mut self) {
        self.input_selection_anchor = None;
    }

    /// Get input selection range as (start, end) byte offsets, where start <= end.
    /// Returns None if no selection.
    pub fn get_input_selection_range(&self) -> Option<(usize, usize)> {
        self.input_selection_anchor.map(|anchor| {
            let start = anchor.min(self.input_cursor);
            let end = anchor.max(self.input_cursor);
            (start, end)
        })
    }

    /// Get selected text from input buffer.
    pub fn get_input_selected_text(&self) -> Option<String> {
        self.get_input_selection_range().map(|(start, end)| {
            self.input_buffer[start..end].to_string()
        })
    }

    /// Delete selected text and return it. Cursor moves to start of selection.
    pub fn delete_input_selection(&mut self) -> Option<String> {
        if let Some((start, end)) = self.get_input_selection_range() {
            if start < end {
                let deleted = self.input_buffer[start..end].to_string();
                self.input_buffer.replace_range(start..end, "");
                self.input_cursor = start;
                self.input_selection_anchor = None;
                return Some(deleted);
            }
        }
        None
    }

    /// Copy selected text to clipboard without deleting.
    pub fn copy_input_selection(&self) -> bool {
        if let Some(text) = self.get_input_selected_text() {
            if !text.is_empty() {
                return copy_to_clipboard(&text);
            }
        }
        false
    }

    /// Cut selected text (copy and delete).
    pub fn cut_input_selection(&mut self) -> bool {
        if self.copy_input_selection() {
            self.delete_input_selection();
            return true;
        }
        false
    }

    /// Move cursor with optional selection extension (for Shift+Arrow).
    /// If `extend_selection` is true, starts or extends selection.
    pub fn move_cursor_with_selection(&mut self, delta: i16, extend_selection: bool) {
        if extend_selection {
            self.start_input_selection();
        } else {
            self.clear_input_selection();
        }
        self.move_cursor(delta);
    }

    /// Select all text in input buffer.
    pub fn select_all_input(&mut self) {
        if !self.input_buffer.is_empty() {
            self.input_selection_anchor = Some(0);
            self.input_cursor = self.input_buffer.len();
        }
    }

    /// Handle text input - if there's a selection, replace it with the new character.
    pub fn insert_char_with_selection(&mut self, c: char) {
        // Delete selection first if present
        self.delete_input_selection();
        // Then insert the character
        self.insert_char(c);
    }

    /// Handle backspace - if there's a selection, delete it.
    pub fn delete_char_with_selection(&mut self) {
        if self.has_input_selection() {
            self.delete_input_selection();
        } else {
            self.delete_char();
        }
    }

    /// Handle delete key - if there's a selection, delete it.
    pub fn delete_char_forward_with_selection(&mut self) {
        if self.has_input_selection() {
            self.delete_input_selection();
        } else {
            self.delete_char_forward();
        }
    }

    /// Set input cursor position from mouse click, optionally extending selection.
    pub fn set_input_cursor_from_click_with_selection(
        &mut self,
        rel_col: u16,
        rel_row: u16,
        extend_selection: bool,
    ) {
        if extend_selection {
            self.start_input_selection();
        } else {
            self.clear_input_selection();
        }
        self.set_input_cursor_from_click(rel_col, rel_row);
    }

    /// Select the word at the current cursor position in the input buffer.
    pub fn select_input_word_at_cursor(&mut self) {
        if self.input_buffer.is_empty() {
            return;
        }

        let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-';

        // Find byte index at cursor position
        let mut byte_idx = 0;
        for (i, c) in self.input_buffer.char_indices() {
            if i >= self.input_cursor {
                break;
            }
            byte_idx = i;
            let _ = c; // Suppress unused warning
        }

        // If cursor is at the end, use the last byte position
        if self.input_cursor >= self.input_buffer.len() && !self.input_buffer.is_empty() {
            byte_idx = self.input_buffer.len();
        }

        let chars: Vec<(usize, char)> = self.input_buffer.char_indices().collect();
        if chars.is_empty() {
            return;
        }

        // Find the character index for the current byte position
        let mut current_char_idx = 0;
        for (idx, (bi, _)) in chars.iter().enumerate() {
            if *bi >= byte_idx {
                current_char_idx = idx;
                break;
            }
            current_char_idx = idx;
        }

        // Get the character at cursor
        let current_char = chars.get(current_char_idx).map(|(_, c)| *c).unwrap_or(' ');

        // If on whitespace, no selection
        if current_char.is_whitespace() {
            return;
        }

        // Find word start
        let mut start_idx = current_char_idx;
        while start_idx > 0 {
            let prev_char = chars.get(start_idx - 1).map(|(_, c)| *c).unwrap_or(' ');
            if !is_word_char(prev_char) {
                break;
            }
            start_idx -= 1;
        }

        // Find word end
        let mut end_idx = current_char_idx;
        while end_idx < chars.len().saturating_sub(1) {
            let next_char = chars.get(end_idx + 1).map(|(_, c)| *c).unwrap_or(' ');
            if !is_word_char(next_char) {
                break;
            }
            end_idx += 1;
        }

        // Convert char indices back to byte indices
        let start_byte = chars.get(start_idx).map(|(bi, _)| *bi).unwrap_or(0);
        let end_byte = chars.get(end_idx).map(|(bi, c)| bi + c.len_utf8()).unwrap_or(self.input_buffer.len());

        // Set selection
        self.input_selection_anchor = Some(start_byte);
        self.input_cursor = end_byte;
    }

    // ========================================================================
    // Scrolling
    // ========================================================================

    /// Scroll the message list by delta lines (negative = up/back, positive = down/forward)
    pub fn scroll(&mut self, delta: i16) {
        if delta < 0 {
            // Scrolling up (into history) - increase offset, but limit to max
            let max_scroll = self.max_scroll_offset.get();
            self.scroll_offset = (self.scroll_offset.saturating_add((-delta) as usize)).min(max_scroll);
        } else {
            // Scrolling down (toward latest) - decrease offset
            self.scroll_offset = self.scroll_offset.saturating_sub(delta as usize);
        }
    }

    /// Scroll to the bottom of the message list
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Check if we're scrolled back (not at bottom)
    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset > 0
    }

    /// Check if there's a message currently being streamed
    pub fn is_streaming(&self) -> bool {
        if let Some(ChatMessage::Assistant { is_streaming, .. }) = self.messages.last() {
            *is_streaming
        } else {
            false
        }
    }

    // ========================================================================
    // Visual Mode
    // ========================================================================

    /// Check if visual mode is active.
    pub fn is_visual_mode(&self) -> bool {
        self.visual_state.is_some()
    }

    /// Check if visual selection is active.
    pub fn is_visual_selecting(&self) -> bool {
        self.visual_state.as_ref().map_or(false, |s| s.is_selecting())
    }

    /// Get visual selection mode.
    pub fn get_visual_selection_mode(&self) -> Option<SelectionMode> {
        self.visual_state.as_ref().map(|s| s.get_selection_mode())
    }

    /// Get visual state reference.
    pub fn visual_state(&self) -> Option<&VisualState> {
        self.visual_state.as_ref()
    }

    /// Enter visual mode.
    /// Initializes the cursor at the bottom-left of the visible message area.
    pub fn enter_visual_mode(&mut self) {
        // If never rendered, place cursor at row 0
        let cached_width = self.cached_visible_width.get();
        if cached_width == 0 {
            self.visual_state = Some(VisualState::new(0, 0));
            return;
        }

        // Compute total_lines fresh to avoid stale cache issues
        // (cached_total_lines is only updated during render)
        let all_lines = self.build_rendered_lines(cached_width as u16);
        let total_lines = all_lines.len();

        // Calculate initial cursor position
        // Use the bottom of visible area, or if no messages, row 0
        let row = if total_lines == 0 {
            0
        } else {
            // Calculate visible bottom in content coordinates
            let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
            visible_bottom.min(total_lines.saturating_sub(1))
        };

        self.visual_state = Some(VisualState::new(row, 0));
    }

    /// Exit visual mode.
    pub fn exit_visual_mode(&mut self) {
        self.visual_state = None;
    }

    /// Move visual cursor by delta.
    /// If cursor moves out of visible area, auto-scroll to keep it visible.
    pub fn move_visual_cursor(&mut self, delta_row: i32, delta_col: i32) {
        let Some(ref mut visual) = self.visual_state else {
            return;
        };

        let total_lines = self.cached_total_lines.get();
        let max_col = self.cached_visible_width.get().saturating_sub(1);
        let max_row = total_lines.saturating_sub(1);

        // Move cursor
        visual.move_cursor(delta_row, delta_col, max_row, max_col);

        // Auto-scroll to keep cursor in view
        self.scroll_to_visual_cursor();
    }

    /// Scroll viewport to ensure visual cursor is visible.
    fn scroll_to_visual_cursor(&mut self) {
        let Some(ref visual) = self.visual_state else {
            return;
        };

        let total_lines = self.cached_total_lines.get();
        let max_scroll = self.max_scroll_offset.get();

        if total_lines == 0 {
            return;
        }

        let (cursor_row, _) = visual.cursor;

        // Calculate visible height (approximation)
        let visible_height = total_lines.saturating_sub(max_scroll);
        if visible_height == 0 {
            return;
        }

        // Calculate visible range in content coordinates
        // scroll_offset = 0 means showing the bottom (most recent)
        // scroll_offset = max_scroll means showing the top (oldest)
        let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
        let visible_top = visible_bottom.saturating_sub(visible_height.saturating_sub(1));

        // Adjust scroll if cursor is out of visible range
        if cursor_row < visible_top {
            // Cursor is above visible area, scroll up
            let new_offset = total_lines.saturating_sub(1).saturating_sub(cursor_row).saturating_sub(visible_height.saturating_sub(1));
            self.scroll_offset = new_offset.min(max_scroll);
        } else if cursor_row > visible_bottom {
            // Cursor is below visible area, scroll down
            self.scroll_offset = total_lines.saturating_sub(1).saturating_sub(cursor_row);
        }
    }

    /// Cycle visual selection mode: None -> Line -> Block -> None
    pub fn cycle_visual_selection(&mut self) {
        if let Some(ref mut visual) = self.visual_state {
            visual.cycle_selection_mode();
        }
    }

    /// Clear visual selection (keep cursor position, reset to None mode).
    pub fn clear_visual_selection(&mut self) {
        if let Some(ref mut visual) = self.visual_state {
            visual.clear_selection();
        }
    }

    /// Start visual selection at current cursor position.
    ///
    /// This enters Line selection mode and sets the anchor to the current cursor.
    /// Used when starting mouse-based selection.
    pub fn start_visual_selection(&mut self) {
        if let Some(ref mut visual) = self.visual_state {
            // If not already selecting, start selection
            if !visual.is_selecting() {
                visual.cycle_selection_mode(); // None -> Line
            }
        }
    }

    /// Set visual cursor position from screen-relative coordinates.
    ///
    /// This is used for mouse-based cursor positioning. The coordinates are
    /// relative to the message area (excluding tab bar and input box).
    ///
    /// # Arguments
    /// * `screen_row` - Row in the visible message area (0 = top of visible content)
    /// * `screen_col` - Column in the visible area
    pub fn set_visual_cursor_from_screen(&mut self, screen_row: usize, screen_col: usize) {
        let Some(ref mut visual) = self.visual_state else {
            return;
        };

        let cached_width = self.cached_visible_width.get();
        if cached_width == 0 {
            return;
        }

        let total_lines = self.cached_total_lines.get();
        if total_lines == 0 {
            return;
        }

        // Calculate visible range and convert screen row to content row
        let max_scroll = self.max_scroll_offset.get();
        let visible_height = total_lines.saturating_sub(max_scroll);
        let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
        let visible_top = visible_bottom.saturating_sub(visible_height.saturating_sub(1));

        let content_row = (visible_top + screen_row).min(total_lines.saturating_sub(1));
        let content_col = screen_col.min(cached_width.saturating_sub(1));

        visual.set_cursor(content_row, content_col);
    }

    /// Select the word at the given screen position.
    ///
    /// This finds word boundaries and sets both anchor and cursor to select the word.
    /// Note: This is a simplified implementation for the Assistant pane. Due to the
    /// complex formatting (wrapped lines, message prefixes), it may not perfectly
    /// match word boundaries in all cases.
    ///
    /// # Arguments
    /// * `screen_row` - Row in the visible message area (0 = top of visible content)
    /// * `screen_col` - Column in the visible area
    pub fn select_word_at(&mut self, screen_row: usize, screen_col: usize) {
        let cached_width = self.cached_visible_width.get();
        if cached_width == 0 {
            return;
        }

        // Build rendered lines to get content
        let all_lines = self.build_rendered_lines(cached_width as u16);
        let total_lines = all_lines.len();

        if total_lines == 0 {
            return;
        }

        // Calculate visible range and convert screen row to content row
        let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
        let max_visible = self.max_scroll_offset.get();
        let visible_height = total_lines.saturating_sub(max_visible);
        let visible_top = visible_bottom.saturating_sub(visible_height.saturating_sub(1));

        let content_row = (visible_top + screen_row).min(total_lines.saturating_sub(1));

        // Get the line content
        let line = &all_lines[content_row];
        let line_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let col = screen_col.min(line_text.chars().count().saturating_sub(1));

        // Find word boundaries
        let (word_start, word_end) = find_word_boundaries_in_string(&line_text, col);

        // Set up visual state with selection
        if let Some(ref mut visual) = self.visual_state {
            // Set anchor at word start
            visual.anchor = Some((content_row, word_start));
            // Set cursor at word end
            visual.set_cursor(content_row, word_end);
            // Ensure we're in Line selection mode
            if !visual.is_selecting() {
                visual.cycle_selection_mode(); // None -> Line
            }
        }
    }

    /// Select the entire line at the given screen position (triple-click).
    ///
    /// This selects from column 0 to the end of the line's content.
    ///
    /// # Arguments
    /// * `screen_row` - Row in the visible message area (0 = top of visible content)
    pub fn select_line_at(&mut self, screen_row: usize) {
        let cached_width = self.cached_visible_width.get();
        if cached_width == 0 {
            return;
        }

        // Build rendered lines to get content
        let all_lines = self.build_rendered_lines(cached_width as u16);
        let total_lines = all_lines.len();

        if total_lines == 0 {
            return;
        }

        // Calculate visible range and convert screen row to content row
        let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
        let max_visible = self.max_scroll_offset.get();
        let visible_height = total_lines.saturating_sub(max_visible);
        let visible_top = visible_bottom.saturating_sub(visible_height.saturating_sub(1));

        let content_row = (visible_top + screen_row).min(total_lines.saturating_sub(1));

        // Get the line content
        let line = &all_lines[content_row];
        let line_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let line_len = line_text.trim_end().chars().count();
        let end_col = line_len.saturating_sub(1);

        // Set up visual state with selection
        if let Some(ref mut visual) = self.visual_state {
            // Set anchor at line start
            visual.anchor = Some((content_row, 0));
            // Set cursor at line end
            visual.set_cursor(content_row, end_col);
            // Ensure we're in Line selection mode
            if !visual.is_selecting() {
                visual.cycle_selection_mode(); // None -> Line
            }
        }
    }

    /// Copy selected text to clipboard and return true if successful.
    /// Line mode: trims trailing spaces from each line.
    /// Block mode: preserves all characters in the rectangle.
    pub fn copy_visual_selection(&mut self) -> bool {
        let Some(ref visual) = self.visual_state else {
            return false;
        };

        let mode = visual.get_selection_mode();
        let Some(((start_row, start_col), (end_row, end_col))) = visual.selection_range() else {
            return false;
        };

        // We need to extract text from the rendered lines
        let text = self.get_text_range(start_row, start_col, end_row, end_col, mode);
        if text.is_empty() {
            return false;
        }

        copy_to_clipboard(&text)
    }

    // ========================================================================
    // Pane Status API (for App to query rendering info)
    // ========================================================================

    /// Get pane status for rendering title bar and hints.
    /// This allows the component to control its appearance without exposing internal state.
    pub fn get_pane_status(&self) -> PaneStatus {
        let mut status_parts: Vec<String> = Vec::new();

        if self.is_visual_mode() {
            if let Some(mode) = self.get_visual_selection_mode() {
                if let Some(name) = mode.display_name() {
                    status_parts.push(name.to_string());
                } else {
                    status_parts.push("VISUAL".to_string());
                }
            } else {
                status_parts.push("VISUAL".to_string());
            }

            // Show repeat count if being accumulated
            if let Some(ref visual) = self.visual_state {
                if let Some(count) = visual.get_repeat_count() {
                    status_parts.push(format!("{}×", count));
                }
            }
        }

        if self.is_scrolled() {
            status_parts.push(format!("Scrolled ↑{}", self.scroll_offset));
        }

        let title_status = if status_parts.is_empty() {
            None
        } else {
            Some(status_parts.join(" "))
        };

        let hint_text = if self.is_visual_mode() {
            Some(" ESC: Exit | Space: Select | y: Copy | hjkl: Move ")
        } else {
            None
        };

        let border_color = if self.is_visual_mode() {
            Some(Color::Magenta)
        } else {
            None
        };

        PaneStatus {
            title_status,
            hint_text,
            border_color,
        }
    }

    /// Handle a key event when in visual mode.
    /// Returns KeyHandleResult indicating how the key was processed.
    pub fn handle_visual_key(&mut self, key: KeyEvent) -> KeyHandleResult {
        if !matches!(key.kind, KeyEventKind::Press) {
            return KeyHandleResult::NotConsumed;
        }

        // If not in visual mode, don't consume
        let Some(ref mut visual) = self.visual_state else {
            return KeyHandleResult::NotConsumed;
        };

        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Ctrl+B => Request command mode
        if ctrl && matches!(key.code, KeyCode::Char('b') | KeyCode::Char('B')) {
            visual.clear_repeat_count();
            return KeyHandleResult::RequestCommandMode;
        }

        match key.code {
            // Digit keys for repeat count
            KeyCode::Char(c @ '1'..='9') => {
                let digit = c.to_digit(10).unwrap_or(0) as usize;
                visual.accumulate_repeat_digit(digit);
                return KeyHandleResult::Consumed;
            }
            KeyCode::Char('0') if visual.has_repeat_count() => {
                visual.accumulate_repeat_digit(0);
                return KeyHandleResult::Consumed;
            }

            // Escape => if in selection mode, go back to VISUAL; if in VISUAL, exit
            KeyCode::Esc => {
                if visual.is_selecting() {
                    // In Line/Block mode, go back to None (VISUAL) mode
                    visual.clear_selection();
                } else {
                    // Already in None mode, exit visual mode
                    self.visual_state = None;
                }
            }

            // Space => toggle selection mode: None -> Line, then Line <-> Block
            KeyCode::Char(' ') => {
                visual.cycle_selection_mode();
            }

            // y => copy selected text and clear selection
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let _ = self.copy_visual_selection();
                if let Some(ref mut v) = self.visual_state {
                    v.clear_selection();
                }
            }

            // Scroll keys (Shift + arrows) - scroll without moving cursor
            KeyCode::Up if shift => {
                let repeat = visual.take_repeat_count();
                for _ in 0..repeat {
                    self.scroll(-1);
                }
            }
            KeyCode::Down if shift => {
                let repeat = visual.take_repeat_count();
                for _ in 0..repeat {
                    self.scroll(1);
                }
            }
            KeyCode::PageUp => {
                let repeat = visual.take_repeat_count();
                self.scroll(-(repeat as i16 * 10));
            }
            KeyCode::PageDown => {
                let repeat = visual.take_repeat_count();
                self.scroll(repeat as i16 * 10);
            }

            // Cursor movement keys (hjkl and arrows)
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('H') => {
                let repeat = visual.take_repeat_count();
                for _ in 0..repeat {
                    self.move_visual_cursor(0, -1);
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let repeat = visual.take_repeat_count();
                for _ in 0..repeat {
                    self.move_visual_cursor(0, 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {
                let repeat = visual.take_repeat_count();
                for _ in 0..repeat {
                    self.move_visual_cursor(-1, 0);
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {
                let repeat = visual.take_repeat_count();
                for _ in 0..repeat {
                    self.move_visual_cursor(1, 0);
                }
            }

            _ => {
                // Unknown key in visual mode - clear repeat count but consume
                if let Some(ref mut v) = self.visual_state {
                    v.clear_repeat_count();
                }
            }
        }

        KeyHandleResult::Consumed
    }

    /// Get text from a range in rendered line coordinates.
    /// Get text from a range in content coordinates.
    /// Line mode: trims trailing spaces from each line.
    /// Block mode: extracts exact rectangle, preserves spaces.
    fn get_text_range(&self, start_row: usize, start_col: usize, end_row: usize, end_col: usize, mode: SelectionMode) -> String {
        // Build all rendered lines (similar to render_message_list)
        let width = self.cached_visible_width.get() as u16;
        if width == 0 {
            return String::new();
        }

        let all_lines = self.build_rendered_lines(width);

        let mut result = String::new();

        for row in start_row..=end_row {
            if row >= all_lines.len() {
                break;
            }

            let line = &all_lines[row];
            let line_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            let chars: Vec<char> = line_text.chars().collect();
            let line_len = chars.len();

            // Determine column range based on mode
            let (col_start, col_end) = match mode {
                SelectionMode::None => continue,
                SelectionMode::Line => {
                    // Line mode: clamp to line length
                    let cs = if row == start_row { start_col } else { 0 };
                    let ce = if row == end_row {
                        end_col.min(line_len.saturating_sub(1))
                    } else {
                        line_len.saturating_sub(1)
                    };
                    (cs, ce)
                }
                SelectionMode::Block => {
                    // Block mode: use exact rectangle columns
                    (start_col, end_col.min(line_len.saturating_sub(1)))
                }
            };

            // Extract characters from this line
            if col_start < line_len {
                let actual_end = col_end.min(line_len.saturating_sub(1));
                if col_start <= actual_end {
                    let line_slice: String = chars[col_start..=actual_end].iter().collect();
                    // For Line mode, trim trailing whitespace
                    // For Block mode, preserve as-is
                    let final_text = match mode {
                        SelectionMode::Line => line_slice.trim_end().to_string(),
                        SelectionMode::Block | SelectionMode::None => line_slice,
                    };
                    result.push_str(&final_text);
                }
            }

            // Add newline between lines (but not after the last line)
            if row < end_row {
                result.push('\n');
            }
        }

        result
    }


    /// Build rendered lines for text extraction (used by visual mode).
    fn build_rendered_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut all_lines: Vec<Line<'static>> = Vec::new();

        for msg in &self.messages {
            match msg {
                ChatMessage::User { text } => {
                    let wrapped = wrap_text_lines(text, width, "You: ");
                    for (i, line) in wrapped.into_iter().enumerate() {
                        if i == 0 {
                            let line_str = line.to_string();
                            let content = line_str.trim_start_matches("You: ").to_string();
                            all_lines.push(Line::from(vec![
                                Span::styled("You: ", Style::default().fg(Color::Green).bold()),
                                Span::raw(content),
                            ]));
                        } else {
                            all_lines.push(line);
                        }
                    }
                    all_lines.push(Line::raw(""));
                }
                ChatMessage::Assistant { text, is_streaming } => {
                    let content = if *is_streaming && text.is_empty() {
                        "...".to_string()
                    } else if *is_streaming {
                        format!("{}▌", text)
                    } else {
                        text.clone()
                    };

                    let wrapped = wrap_text_lines(&content, width, "AI: ");
                    for (i, line) in wrapped.into_iter().enumerate() {
                        if i == 0 {
                            let line_str = line.to_string();
                            let content = line_str.trim_start_matches("AI: ").to_string();
                            all_lines.push(Line::from(vec![
                                Span::styled("AI: ", Style::default().fg(Color::Cyan).bold()),
                                Span::raw(content),
                            ]));
                        } else {
                            all_lines.push(line);
                        }
                    }
                    if !text.is_empty() || *is_streaming {
                        all_lines.push(Line::raw(""));
                    }
                }
                ChatMessage::CommandCard {
                    command,
                    explanation,
                    status,
                    verdict,
                } => {
                    // Show pagination only for pending commands
                    let pagination = if *status == CommandStatus::Pending {
                        self.suggestion_pagination()
                    } else {
                        None
                    };
                    all_lines.extend(render_command_card(
                        command,
                        explanation,
                        *status,
                        verdict,
                        width,
                        pagination
                    ));
                    all_lines.push(Line::raw(""));
                }
                ChatMessage::Error { text } => {
                    let wrapped = wrap_text_lines(text, width, "⚠ ");
                    for (i, line) in wrapped.into_iter().enumerate() {
                        if i == 0 {
                            let line_str = line.to_string();
                            let content = line_str.trim_start_matches("⚠ ").to_string();
                            all_lines.push(Line::from(vec![
                                Span::styled("⚠ ", Style::default().fg(Color::Red).bold()),
                                Span::styled(content, Style::default().fg(Color::Red)),
                            ]));
                        } else {
                            all_lines.push(Line::styled(
                                line.to_string(),
                                Style::default().fg(Color::Red),
                            ));
                        }
                    }
                    all_lines.push(Line::raw(""));
                }
            }
        }

        all_lines
    }

    /// Get visual cursor position in screen coordinates (for rendering).
    /// Returns (screen_row, screen_col) if cursor is visible, None otherwise.
    pub fn get_visual_cursor_screen_pos(&self) -> Option<(usize, usize)> {
        let visual = self.visual_state.as_ref()?;
        let (cursor_row, cursor_col) = visual.cursor;

        let total_lines = self.cached_total_lines.get();
        let max_scroll = self.max_scroll_offset.get();

        if total_lines == 0 {
            return Some((0, cursor_col));
        }

        // Calculate visible height
        let visible_height = total_lines.saturating_sub(max_scroll);
        if visible_height == 0 {
            return None;
        }

        // Calculate visible range
        let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
        let visible_top = visible_bottom.saturating_sub(visible_height.saturating_sub(1));

        // Check if cursor is in visible range
        if cursor_row >= visible_top && cursor_row <= visible_bottom {
            let screen_row = cursor_row - visible_top;
            Some((screen_row, cursor_col))
        } else {
            None
        }
    }

    /// Convert screen row to content row for selection checking.
    fn screen_row_to_content_row(&self, screen_row: usize) -> usize {
        let total_lines = self.cached_total_lines.get();
        let max_scroll = self.max_scroll_offset.get();

        if total_lines == 0 {
            return 0;
        }

        let visible_height = total_lines.saturating_sub(max_scroll);
        let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
        let visible_top = visible_bottom.saturating_sub(visible_height.saturating_sub(1));

        visible_top + screen_row
    }

    /// Calculate the number of lines needed to display the input text.
    /// Returns the number of lines (minimum 1).
    pub fn calculate_input_lines(&self, width: u16) -> u16 {
        if width == 0 {
            return 1;
        }

        let prompt_width = self.prompt_width();

        if self.input_buffer.is_empty() {
            return 1;
        }

        let mut lines = 1u16;
        let mut current_x = prompt_width;

        for ch in self.input_buffer.chars() {
            if ch == '\n' {
                // Manual newline: start a new line
                lines += 1;
                current_x = 0;
                continue;
            }

            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;

            if current_x + char_width > width {
                // Wrap to next line
                lines += 1;
                current_x = char_width;
            } else {
                current_x += char_width;
            }
        }

        lines
    }

    /// Move cursor up one line in multi-line input
    pub fn move_cursor_up(&mut self, input_area_width: u16) {
        if input_area_width == 0 {
            return;
        }

        let prompt_width = self.prompt_width();
        let before_cursor = &self.input_buffer[..self.input_cursor];

        // Track line starts and calculate current screen column
        let mut current_line_start = 0;
        let mut prev_line_start = 0;
        let mut x = prompt_width; // First line starts after prompt
        let mut byte_pos = 0;

        for ch in before_cursor.chars() {
            if ch == '\n' {
                prev_line_start = current_line_start;
                current_line_start = byte_pos + ch.len_utf8();
                x = 0; // Lines after first start at column 0
            } else {
                let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
                if x + char_width > input_area_width {
                    // Auto-wrap
                    prev_line_start = current_line_start;
                    current_line_start = byte_pos;
                    x = char_width;
                } else {
                    x += char_width;
                }
            }
            byte_pos += ch.len_utf8();
        }

        // Current screen column is the current x position
        let current_screen_col = x;

        // If we're on the very first line (no wrap, no newline), do nothing
        if current_line_start == 0 {
            return;
        }

        // Get the text of the previous line
        let prev_line_text = if current_line_start > 0 {
            // Check if there's a newline before current line
            let before_current = &self.input_buffer[..current_line_start];
            if before_current.ends_with('\n') {
                &self.input_buffer[prev_line_start..current_line_start - 1]
            } else {
                &self.input_buffer[prev_line_start..current_line_start]
            }
        } else {
            ""
        };

        // Determine starting x for previous line
        // If prev_line_start is 0, it's the first line (starts with prompt)
        // Otherwise, it starts at x=0
        let prev_line_start_x = if prev_line_start == 0 {
            prompt_width
        } else {
            0
        };

        // Scan previous line to find the position at target screen column
        let mut target_x = prev_line_start_x;
        let mut target_byte_pos = prev_line_start;

        for ch in prev_line_text.chars() {
            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
            if target_x + char_width > input_area_width {
                break; // Stop at wrap point (shouldn't happen in a line segment)
            }
            if target_x >= current_screen_col {
                break; // Reached target column
            }
            target_x += char_width;
            target_byte_pos += ch.len_utf8();
        }

        self.input_cursor = target_byte_pos;
    }

    /// Move cursor down one line in multi-line input
    pub fn move_cursor_down(&mut self, input_area_width: u16) {
        if input_area_width == 0 {
            return;
        }

        let prompt_width = self.prompt_width();
        let before_cursor = &self.input_buffer[..self.input_cursor];
        let after_cursor = &self.input_buffer[self.input_cursor..];

        // Find current line start and calculate screen column (x position)
        let mut x = prompt_width; // First line starts after prompt
        let mut byte_pos = 0;

        for ch in before_cursor.chars() {
            if ch == '\n' {
                x = 0; // Lines after first start at column 0
            } else {
                let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
                if x + char_width > input_area_width {
                    x = char_width;
                } else {
                    x += char_width;
                }
            }
            byte_pos += ch.len_utf8();
        }

        // Current screen column is the current x position
        let current_screen_col = x;

        // Find the next line start from current cursor position
        let mut next_line_start = None;
        let mut next_next_line_start = None;
        // Continue from current x position
        byte_pos = self.input_cursor;

        for ch in after_cursor.chars() {
            if ch == '\n' {
                if next_line_start.is_none() {
                    next_line_start = Some(byte_pos + ch.len_utf8());
                    x = 0;
                } else {
                    next_next_line_start = Some(byte_pos + ch.len_utf8());
                    break;
                }
            } else {
                let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
                if x + char_width > input_area_width {
                    if next_line_start.is_none() {
                        next_line_start = Some(byte_pos);
                        x = char_width;
                    } else {
                        next_next_line_start = Some(byte_pos);
                        break;
                    }
                } else {
                    x += char_width;
                }
            }
            byte_pos += ch.len_utf8();
        }

        // If there's no next line, do nothing
        let Some(next_start) = next_line_start else {
            return;
        };

        // Find position in next line that matches current screen column
        let next_line_text = if let Some(next_next) = next_next_line_start {
            &self.input_buffer[next_start..next_next]
        } else {
            &self.input_buffer[next_start..]
        };

        // Scan next line to find the position at target screen column
        let mut target_x = 0u16;
        let mut target_byte_pos = next_start;

        for ch in next_line_text.chars() {
            if ch == '\n' {
                break; // Stop at newline
            }
            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
            if target_x + char_width > input_area_width {
                break; // Stop at wrap point
            }
            if target_x >= current_screen_col {
                break; // Reached target column
            }
            target_x += char_width;
            target_byte_pos += ch.len_utf8();
        }

        self.input_cursor = target_byte_pos;
    }

    /// Calculate the actual input box height (including border) for a given area height and width.
    pub fn calculate_input_box_height(&self, area_height: u16, area_width: u16) -> u16 {
        let input_text_lines = self.calculate_input_lines(area_width);
        let min_input_height = 3u16;
        let max_input_height = (area_height / 2).max(min_input_height);
        (input_text_lines + 1).clamp(min_input_height, max_input_height)
    }

    // ========================================================================
    // Mouse Click Detection
    // ========================================================================

    /// Detect what was clicked in the tab bar.
    ///
    /// # Arguments
    /// * `rel_col` - Column position relative to the assistant inner area
    /// * `area_x` - X offset of the assistant inner area (for screen coordinate conversion)
    ///
    /// Returns a TabClickResult indicating what action to take.
    pub fn get_tab_click_result(&self, screen_col: u16, area_x: u16) -> TabClickResult {
        let tab_positions = self.cached_tab_positions.borrow();

        // Convert screen column to absolute position for comparison
        let abs_col = screen_col;

        for hit in tab_positions.iter() {
            let start = area_x + hit.start_x;
            let end = area_x + hit.end_x;

            if abs_col >= start && abs_col < end {
                if hit.is_close_button {
                    if let Some(id) = hit.session_id {
                        return TabClickResult::CloseTab(id);
                    }
                } else if let Some(id) = hit.session_id {
                    // Regular tab click - switch to it
                    return TabClickResult::SwitchToTab(id);
                } else {
                    // "+" button
                    return TabClickResult::NewTab;
                }
            }
        }

        TabClickResult::None
    }

    /// Detect what was clicked in the message area.
    ///
    /// # Arguments
    /// * `screen_col` - Screen column position
    /// * `screen_row` - Screen row position
    /// * `area` - The message area rect (for coordinate conversion)
    ///
    /// Returns a MessageAreaClickResult indicating what action to take.
    pub fn get_message_click_result(
        &self,
        screen_col: u16,
        screen_row: u16,
        area_x: u16,
        area_y: u16,
    ) -> MessageAreaClickResult {
        let command_cards = self.cached_command_cards.borrow();

        for card in command_cards.iter() {
            // Check if click is on the button row
            if let Some(button_y) = card.button_y {
                let button_screen_y = area_y + button_y;

                if screen_row == button_screen_y {
                    // Check next button (pagination)
                    if let Some((btn_start, btn_end)) = card.next_btn {
                        let btn_start_x = area_x + btn_start;
                        let btn_end_x = area_x + btn_end;
                        if screen_col >= btn_start_x && screen_col < btn_end_x {
                            return MessageAreaClickResult::NextCommand(card.message_idx);
                        }
                    }

                    // Check execute button
                    if let Some((btn_start, btn_end)) = card.execute_btn {
                        let btn_start_x = area_x + btn_start;
                        let btn_end_x = area_x + btn_end;
                        if screen_col >= btn_start_x && screen_col < btn_end_x {
                            return MessageAreaClickResult::ExecuteCommand(card.message_idx);
                        }
                    }

                    // Check cancel button
                    if let Some((btn_start, btn_end)) = card.cancel_btn {
                        let btn_start_x = area_x + btn_start;
                        let btn_end_x = area_x + btn_end;
                        if screen_col >= btn_start_x && screen_col < btn_end_x {
                            return MessageAreaClickResult::CancelCommand(card.message_idx);
                        }
                    }
                }
            }
        }

        MessageAreaClickResult::None
    }

    /// Set the input cursor position based on screen click coordinates.
    ///
    /// # Arguments
    /// * `rel_col` - Column relative to the input box inner area
    /// * `rel_row` - Row relative to the input box inner area
    pub fn set_input_cursor_from_click(&mut self, rel_col: u16, rel_row: u16) {
        let input_area_width = self.last_input_area_width.get();
        if input_area_width == 0 {
            return;
        }

        let prompt_width = self.prompt_width();

        // Track position through the input buffer
        let mut current_x = prompt_width;
        let mut current_y = 0u16;
        let mut byte_pos = 0usize;
        let mut best_pos = 0usize;

        for ch in self.input_buffer.chars() {
            // Check if we've reached or passed the target position
            if current_y > rel_row || (current_y == rel_row && current_x >= rel_col) {
                break;
            }
            best_pos = byte_pos + ch.len_utf8();

            if ch == '\n' {
                // Manual newline
                current_y += 1;
                current_x = 0;
            } else {
                let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;

                if current_x + char_width > input_area_width {
                    // Wrap to next line
                    current_y += 1;
                    current_x = char_width;
                } else {
                    current_x += char_width;
                }
            }

            byte_pos += ch.len_utf8();
        }

        // If click is beyond the last character, position at end
        if current_y < rel_row || (current_y == rel_row && current_x < rel_col) {
            best_pos = self.input_buffer.len();
        }

        self.input_cursor = best_pos;
    }

    /// Get cursor position for rendering.
    /// Returns Some((x, y)) if cursor should be shown, None otherwise.
    /// Coordinates are relative to the input box inner area.
    pub fn get_cursor_position(&self) -> Option<(u16, u16)> {
        let prompt_width = self.prompt_width();
        let input_area_width = self.input_area_width();

        // Calculate available width for text (excluding prompt)
        let available_width = input_area_width.saturating_sub(prompt_width);
        if available_width == 0 {
            return None;
        }

        // Get text before cursor
        let before_cursor = &self.input_buffer[..self.input_cursor];

        // Calculate cursor position considering line wrapping and newlines
        let mut x = prompt_width;
        let mut y = 0u16;

        for ch in before_cursor.chars() {
            if ch == '\n' {
                // Manual newline: move to start of next line
                x = 0;
                y += 1;
                continue;
            }

            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;

            if x + char_width > input_area_width {
                // Wrap to next line before adding this character
                x = char_width;
                y += 1;
            } else {
                x += char_width;
            }
        }

        // If cursor is exactly at the right edge, check if we should wrap
        // Only wrap if there's more content after the cursor (not at the end)
        if x >= input_area_width && self.input_cursor < self.input_buffer.len() {
            x = 0;
            y += 1;
        }

        Some((x, y))
    }
}


// ============================================================================
// Widget Implementation
// ============================================================================

impl Widget for &TuiAssistant {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Calculate dynamic input box height based on content
        let input_box_height = self.calculate_input_box_height(area.height, area.width);

        // Split into three regions: tabs (1 line), messages (flexible), input (dynamic)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),              // Tab bar
                Constraint::Min(1),                 // Message list
                Constraint::Length(input_box_height), // Input box (dynamic)
            ])
            .split(area);

        // Render each section
        render_tab_bar(self, chunks[0], buf);
        render_message_list(self, chunks[1], buf);
        render_input_box(self, chunks[2], buf);
    }
}

// ============================================================================
// Rendering Functions
// ============================================================================

// Tab bar configuration constants
const TAB_PADDING: usize = 2;       // " name " -> 2 spaces around name
const TAB_SEPARATOR: usize = 1;     // Space between tabs
const TAB_PLUS_BUTTON_WIDTH: usize = 4; // " + " with leading space

/// Generate a short tab name like "S1", "S2" from the tab name.
/// Extracts the number from the name, or uses the provided index + 1.
fn get_short_tab_name(name: &str, index: usize) -> String {
    // Try to extract number from name (e.g., "Session 3" -> "3")
    let num: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
    if num.is_empty() {
        format!("S{}", index + 1)
    } else {
        format!("S{}", num)
    }
}

/// Calculate the display width of a tab.
/// If `use_short` is true, uses short form (S1, S2); otherwise uses full name.
fn calculate_tab_width(name: &str, index: usize, use_short: bool) -> usize {
    let display_name = if use_short {
        get_short_tab_name(name, index)
    } else {
        name.to_string()
    };
    TAB_PADDING + unicode_width::UnicodeWidthStr::width(display_name.as_str())
}

/// Calculate total width for tabs in a range.
/// Active tab always uses full name, others use short form if `others_short` is true.
fn calculate_tabs_width_mixed(
    tabs: &[SessionTab],
    range: std::ops::Range<usize>,
    active_idx: usize,
    others_short: bool,
) -> usize {
    let mut total = 0;
    for (i, idx) in range.clone().enumerate() {
        if i > 0 {
            total += TAB_SEPARATOR;
        }
        let use_short = others_short && idx != active_idx;
        total += calculate_tab_width(&tabs[idx].name, idx, use_short);
    }
    total
}

/// Check if all tabs fit with full names.
fn all_tabs_fit_full(tabs: &[SessionTab], active_idx: usize, available_width: usize) -> bool {
    calculate_tabs_width_mixed(tabs, 0..tabs.len(), active_idx, false) <= available_width
}

/// Check if all tabs fit with active full, others short.
fn all_tabs_fit_mixed(tabs: &[SessionTab], active_idx: usize, available_width: usize) -> bool {
    calculate_tabs_width_mixed(tabs, 0..tabs.len(), active_idx, true) <= available_width
}

/// Calculate visible tab range centered around active tab.
/// Active tab uses full name, others use short form.
/// Returns (start_idx, end_idx, hidden_left, hidden_right).
fn calculate_visible_range(
    tabs: &[SessionTab],
    active_idx: usize,
    available_width: usize,
) -> (usize, usize, usize, usize) {
    let num_tabs = tabs.len();
    if num_tabs == 0 {
        return (0, 0, 0, 0);
    }

    // Width of hidden indicators: "‹N " and " N›" (each ~3-4 chars)
    let indicator_width = 4;

    // Start with just the active tab (full name)
    let mut start = active_idx;
    let mut end = active_idx + 1;

    // Helper to calculate current width including potential indicators
    let calc_width = |s: usize, e: usize| -> usize {
        let mut w = calculate_tabs_width_mixed(tabs, s..e, active_idx, true);
        if s > 0 {
            w += indicator_width;
        }
        if e < num_tabs {
            w += indicator_width;
        }
        w
    };

    // Expand alternately left and right while we have space
    loop {
        let mut expanded = false;

        // Try to expand right
        if end < num_tabs {
            let new_width = calc_width(start, end + 1);
            if new_width <= available_width {
                end += 1;
                expanded = true;
            }
        }

        // Try to expand left
        if start > 0 {
            let new_width = calc_width(start - 1, end);
            if new_width <= available_width {
                start -= 1;
                expanded = true;
            }
        }

        if !expanded {
            break;
        }
    }

    (start, end, start, num_tabs - end)
}

/// Render the session tab bar with overflow handling.
///
/// Strategy:
/// 1. Try to fit all tabs with full names
/// 2. If not, show active tab with full name, others with short form (S1, S2, ...)
/// 3. If still doesn't fit, show only tabs around active one with hidden indicators
fn render_tab_bar(assistant: &TuiAssistant, area: Rect, buf: &mut Buffer) {
    let mut spans = Vec::new();
    let tabs = &assistant.session_tabs;
    let num_tabs = tabs.len();

    // Clear and prepare to track hit areas
    let mut hit_areas: Vec<TabHitArea> = Vec::new();
    let mut current_x: u16 = 0;

    if num_tabs == 0 {
        // Just show the "+" button
        let plus_text = " + ";
        hit_areas.push(TabHitArea {
            start_x: current_x,
            end_x: current_x + plus_text.len() as u16,
            session_id: None,
            is_close_button: false,
        });
        spans.push(Span::styled(plus_text, Style::default().fg(Color::Green)));
        let line = Line::from(spans);
        Paragraph::new(line).render(area, buf);
        *assistant.cached_tab_positions.borrow_mut() = hit_areas;
        return;
    }

    // Find the index of the active session
    let active_idx = tabs
        .iter()
        .position(|t| t.id == assistant.active_session)
        .unwrap_or(0);

    // Calculate available width for tabs (excluding "+" button, and account for close button on active)
    let total_width = area.width as usize;
    // Active tab will have " x" suffix (2 chars), so account for that
    let close_button_width = 2;
    let available_for_tabs = total_width.saturating_sub(TAB_PLUS_BUTTON_WIDTH + close_button_width);

    // Determine display mode:
    // - Mode 0: All tabs with full names
    // - Mode 1: Active full, others short (S1, S2, ...)
    // - Mode 2: Only visible range around active, with hidden indicators
    let (visible_start, visible_end, hidden_left, hidden_right, use_short_for_others) =
        if all_tabs_fit_full(tabs, active_idx, available_for_tabs) {
            // All tabs fit with full names
            (0, num_tabs, 0, 0, false)
        } else if all_tabs_fit_mixed(tabs, active_idx, available_for_tabs) {
            // All tabs fit with active full, others short
            (0, num_tabs, 0, 0, true)
        } else {
            // Need to hide some tabs
            let (start, end, left, right) =
                calculate_visible_range(tabs, active_idx, available_for_tabs);
            (start, end, left, right, true)
        };

    // Render left hidden indicator
    if hidden_left > 0 {
        let indicator = format!("‹{} ", hidden_left);
        current_x += indicator.width() as u16;
        spans.push(Span::styled(
            indicator,
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Find next tab index for Tab hint (within visible range)
    let next_idx_global = (active_idx + 1) % num_tabs;
    let next_in_visible = if next_idx_global >= visible_start && next_idx_global < visible_end {
        Some(next_idx_global)
    } else {
        None
    };

    // Render visible tabs
    for (render_idx, tab_idx) in (visible_start..visible_end).enumerate() {
        let tab = &tabs[tab_idx];

        if render_idx > 0 {
            spans.push(Span::raw(" "));
            current_x += 1;
        }

        let is_active = tab.id == assistant.active_session;
        let is_next = Some(tab_idx) == next_in_visible && num_tabs > 1;

        // Add Tab hint before the next session (subtle indicator)
        if is_next && !is_active {
            spans.push(Span::styled("⇥", Style::default().fg(Color::DarkGray)));
            current_x += 1;
        }

        let style = if is_active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        // Active tab always shows full name, others may be shortened
        let display_name = if is_active || !use_short_for_others {
            tab.name.clone()
        } else {
            get_short_tab_name(&tab.name, tab_idx)
        };

        let tab_text = format!(" {} ", display_name);
        let tab_text_width = tab_text.width() as u16;

        // Track hit area for the tab itself
        let tab_start_x = current_x;
        spans.push(Span::styled(tab_text, style));
        current_x += tab_text_width;

        // For active tab, add close button "x"
        if is_active {
            let close_btn_start = current_x;
            let close_style = Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(180, 40, 40))
                .add_modifier(Modifier::BOLD);
            spans.push(Span::styled("×", close_style));
            current_x += 1;

            // Track tab area (excluding close button)
            hit_areas.push(TabHitArea {
                start_x: tab_start_x,
                end_x: close_btn_start,
                session_id: Some(tab.id),
                is_close_button: false,
            });

            // Track close button area
            hit_areas.push(TabHitArea {
                start_x: close_btn_start,
                end_x: current_x,
                session_id: Some(tab.id),
                is_close_button: true,
            });
        } else {
            // Non-active tab - just the tab area
            hit_areas.push(TabHitArea {
                start_x: tab_start_x,
                end_x: current_x,
                session_id: Some(tab.id),
                is_close_button: false,
            });
        }
    }

    // Render right hidden indicator
    if hidden_right > 0 {
        let indicator = format!(" {}›", hidden_right);
        current_x += indicator.width() as u16;
        spans.push(Span::styled(
            indicator,
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Add "+" button for new session (Ctrl+B,T in command mode)
    spans.push(Span::raw(" "));
    current_x += 1;

    let plus_start = current_x;
    let plus_text = " + ";
    spans.push(Span::styled(plus_text, Style::default().fg(Color::Green)));
    current_x += plus_text.len() as u16;

    hit_areas.push(TabHitArea {
        start_x: plus_start,
        end_x: current_x,
        session_id: None,
        is_close_button: false,
    });

    // Store hit areas for click detection
    *assistant.cached_tab_positions.borrow_mut() = hit_areas;

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    paragraph.render(area, buf);
}

/// Wrap text to fit within a given width, returning multiple lines.
/// Uses the `textwrap` crate for intelligent word-boundary wrapping.
///
/// This function handles text wrapping to ensure accurate line counting
/// for scrolling calculations. We can't use Paragraph.wrap() because:
///
/// 1. Paragraph.wrap() only executes during render(), but we need line counts BEFORE rendering
/// 2. Ratatui doesn't provide a public API to pre-calculate wrapped line counts
/// 3. Using textwrap gives us smart word-boundary wrapping and precise line counting
fn wrap_text_lines(text: &str, width: u16, prefix: &str) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from(text.to_string())];
    }

    let mut result_lines = Vec::new();
    let prefix_width = unicode_width::UnicodeWidthStr::width(prefix);

    // Split by manual newlines first
    let paragraphs: Vec<&str> = text.split('\n').collect();

    for (para_idx, paragraph) in paragraphs.iter().enumerate() {
        if para_idx == 0 {
            // First paragraph: first line has prefix, subsequent lines are indented
            if paragraph.is_empty() {
                result_lines.push(Line::from(prefix.to_string()));
            } else {
                let first_line_width = (width as usize).saturating_sub(prefix_width);
                let wrapped = textwrap::wrap(paragraph, first_line_width.max(10));

                for (i, line) in wrapped.iter().enumerate() {
                    if i == 0 {
                        // First line with prefix
                        result_lines.push(Line::from(format!("{}{}", prefix, line)));
                    } else {
                        // Continuation lines with indentation
                        result_lines.push(Line::from(format!("{:width$}{}", "", line, width = prefix_width)));
                    }
                }
            }
        } else {
            // Subsequent paragraphs (after manual newlines): all lines are indented
            if paragraph.is_empty() {
                // Empty line (from consecutive newlines)
                result_lines.push(Line::from(format!("{:width$}", "", width = prefix_width)));
            } else {
                let wrap_width = (width as usize).saturating_sub(prefix_width);
                let wrapped = textwrap::wrap(paragraph, wrap_width.max(10));
                for line in wrapped {
                    result_lines.push(Line::from(format!("{:width$}{}", "", line, width = prefix_width)));
                }
            }
        }
    }

    // Ensure we have at least one line
    if result_lines.is_empty() {
        result_lines.push(Line::from(prefix.to_string()));
    }

    result_lines
}

/// Render the message list area
fn render_message_list(assistant: &TuiAssistant, area: Rect, buf: &mut Buffer) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    // Build all lines from messages and track command card positions
    let mut all_lines: Vec<Line> = Vec::new();
    // Track: (message_idx, start_line_idx, card_height, is_pending, has_pagination)
    let mut card_line_ranges: Vec<(usize, usize, usize, bool, bool)> = Vec::new();

    for (msg_idx, msg) in assistant.messages.iter().enumerate() {
        match msg {
            ChatMessage::User { text } => {
                // Manually wrap user message text
                let wrapped = wrap_text_lines(text, area.width, "You: ");
                for (i, line) in wrapped.into_iter().enumerate() {
                    if i == 0 {
                        // First line with styled prefix
                        let line_str = line.to_string();
                        let content = line_str.trim_start_matches("You: ").to_string();
                        all_lines.push(Line::from(vec![
                            Span::styled("You: ", Style::default().fg(Color::Green).bold()),
                            Span::raw(content),
                        ]));
                    } else {
                        // Continuation lines
                        all_lines.push(line);
                    }
                }
                all_lines.push(Line::raw("")); // Empty line after message
            }
            ChatMessage::Assistant { text, is_streaming } => {
                // Prepare content with streaming indicator
                let content = if *is_streaming && text.is_empty() {
                    "...".to_string()
                } else if *is_streaming {
                    format!("{}▌", text) // Add cursor for streaming
                } else {
                    text.clone()
                };

                // Manually wrap assistant message text
                let wrapped = wrap_text_lines(&content, area.width, "AI: ");
                for (i, line) in wrapped.into_iter().enumerate() {
                    if i == 0 {
                        // First line with styled prefix
                        let line_str = line.to_string();
                        let content = line_str.trim_start_matches("AI: ").to_string();
                        all_lines.push(Line::from(vec![
                            Span::styled("AI: ", Style::default().fg(Color::Cyan).bold()),
                            Span::raw(content),
                        ]));
                    } else {
                        // Continuation lines
                        all_lines.push(line);
                    }
                }
                // Only add empty line if message has content (skip for empty placeholder before command cards)
                if !text.is_empty() || *is_streaming {
                    all_lines.push(Line::raw("")); // Empty line after message
                }
            }
            ChatMessage::CommandCard {
                command,
                explanation,
                status,
                verdict,
            } => {
                // Show pagination only for pending commands
                let pagination = if *status == CommandStatus::Pending {
                    assistant.suggestion_pagination()
                } else {
                    None
                };
                let start_line = all_lines.len();
                let is_pending = *status == CommandStatus::Pending;
                let card_lines = render_command_card(
                    command,
                    explanation,
                    *status,
                    verdict,
                    area.width,
                    pagination
                );
                let card_height = card_lines.len();
                all_lines.extend(card_lines);
                all_lines.push(Line::raw("")); // Empty line after card

                // Track this card's position (include pagination info for button hit area calculation)
                card_line_ranges.push((msg_idx, start_line, card_height, is_pending, pagination.is_some()));
            }
            ChatMessage::Error { text } => {
                // Render error message with distinct styling
                let wrapped = wrap_text_lines(text, area.width, "⚠ ");
                for (i, line) in wrapped.into_iter().enumerate() {
                    if i == 0 {
                        let line_str = line.to_string();
                        let content = line_str.trim_start_matches("⚠ ").to_string();
                        all_lines.push(Line::from(vec![
                            Span::styled("⚠ ", Style::default().fg(Color::Red).bold()),
                            Span::styled(content, Style::default().fg(Color::Red)),
                        ]));
                    } else {
                        all_lines.push(Line::styled(
                            line.to_string(),
                            Style::default().fg(Color::Red),
                        ));
                    }
                }
                all_lines.push(Line::raw("")); // Empty line after error
            }
        }
    }

    // Calculate scroll offset (0 = at bottom, >0 = scrolled up)
    let total_lines = all_lines.len();
    let visible_lines = area.height as usize;

    // Clamp scroll_offset to valid range
    let max_scroll = total_lines.saturating_sub(visible_lines);

    // Cache values for visual mode
    assistant.max_scroll_offset.set(max_scroll);
    assistant.cached_total_lines.set(total_lines);
    assistant.cached_visible_width.set(area.width as usize);

    let effective_scroll = assistant.scroll_offset.min(max_scroll);

    // Calculate which lines to show
    // When scroll_offset = 0, we show the last N lines (at bottom)
    // When scroll_offset = max, we show the first N lines (at top)
    let skip = total_lines.saturating_sub(visible_lines + effective_scroll);

    // Get visual mode state for highlighting
    let visual_cursor_pos = assistant.get_visual_cursor_screen_pos();
    let selection_range = assistant.visual_state.as_ref().and_then(|v| v.selection_range());
    let selection_mode = assistant.visual_state.as_ref().map(|v| v.get_selection_mode()).unwrap_or(SelectionMode::None);

    // Render with visual mode support
    let visible: Vec<Line> = all_lines.into_iter().skip(skip).take(visible_lines).collect();

    // Build line widths for selection clamping (for Line mode)
    // We collect the effective width (trimmed) for each visible line's content row
    let line_widths: Vec<usize> = visible.iter().map(|line| {
        let line_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        line_text.trim_end().chars().count()
    }).collect();

    // Render lines with visual mode highlighting
    for (screen_row, line) in visible.iter().enumerate() {
        if screen_row >= area.height as usize {
            break;
        }

        let y = area.y + screen_row as u16;
        let mut x = area.x;
        let mut col: usize = 0;

        for span in &line.spans {
            for c in span.content.chars() {
                if x >= area.x + area.width {
                    break;
                }

                let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as u16;

                // Determine style with visual mode modifications
                let mut style = span.style;

                // Check if this cell is the visual cursor
                let is_cursor = visual_cursor_pos.map_or(false, |(cr, cc)| cr == screen_row && cc == col);

                // Check if this cell is in the selection range
                let is_selected = selection_range.map_or(false, |(start, end)| {
                    let content_row = assistant.screen_row_to_content_row(screen_row);
                    is_in_selection_with_mode(
                        content_row,
                        col,
                        start,
                        end,
                        selection_mode,
                        |r| {
                            // Map content row to screen row to get the line width
                            let visible_top = total_lines.saturating_sub(visible_lines + effective_scroll);
                            if r >= visible_top && r < visible_top + line_widths.len() {
                                line_widths[r - visible_top]
                            } else {
                                // Row not in visible area, use cached width as fallback
                                assistant.cached_visible_width.get()
                            }
                        },
                    )
                });

                if is_cursor {
                    // Visual cursor: blue background, white foreground
                    style = Style::default().fg(Color::White).bg(Color::Blue);
                } else if is_selected {
                    // Selection: blue background, white foreground
                    style = Style::default().fg(Color::White).bg(Color::Blue);
                }

                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(c).set_style(style);
                }
                x += char_width;
                col += 1;
            }
        }

        // If cursor is on this row but beyond the rendered content, render it
        if let Some((cursor_row, cursor_col)) = visual_cursor_pos {
            if cursor_row == screen_row && cursor_col >= col {
                let cursor_x = area.x + cursor_col as u16;
                if cursor_x < area.x + area.width {
                    let style = Style::default().fg(Color::White).bg(Color::Blue);
                    if let Some(cell) = buf.cell_mut((cursor_x, y)) {
                        cell.set_char(' ').set_style(style);
                    }
                }
            }
        }
    }

    // Calculate command card hit areas for mouse click detection
    // Convert content line indices to screen coordinates
    let mut command_card_hits: Vec<CommandCardHitArea> = Vec::new();

    for (msg_idx, start_line, card_height, is_pending, has_pagination) in card_line_ranges {
        // Check if any part of this card is visible
        let card_end_line = start_line + card_height;

        // Skip lines before visible area
        if card_end_line <= skip {
            continue;
        }
        // Skip lines after visible area
        if start_line >= skip + visible_lines {
            continue;
        }

        // Calculate screen Y coordinates (relative to area)
        let screen_start_y = if start_line >= skip {
            (start_line - skip) as u16
        } else {
            0
        };
        let screen_end_y = ((card_end_line - skip).min(visible_lines)) as u16;

        // Only track button positions for pending cards
        let (button_y, next_btn, execute_btn, cancel_btn) = if is_pending {
            // Card layout (with verdict line):
            // Line 0: Top border
            // Line 1: Verdict line
            // Line 2: Explanation line (optional)
            // Line 3/2: Command line
            // Line 4/3: Status/action line with buttons
            // Line 5/4: Bottom border
            //
            // Button line is always the second-to-last line (card_height - 2)
            let button_line_offset = card_height.saturating_sub(2);
            let button_screen_y = screen_start_y + button_line_offset as u16;

            if button_screen_y < screen_end_y {
                // Button positions based on actual rendered spans:
                // " │" = 2 chars
                //
                // With pagination:
                //   " [Ctrl+A] Next " (15 chars at 2-16)
                //   " " (1 char)
                //   " [Ctrl+Y] Execute " (18 chars at 18-35)
                //   " " (1 char)
                //   " [Ctrl+N] Cancel " (17 chars at 37-53)
                //
                // Without pagination:
                //   " [Ctrl+Y] Execute " (18 chars at 2-19)
                //   " " (1 char)
                //   " [Ctrl+N] Cancel " (17 chars at 21-37)
                let (next_btn, exec_start, exec_end, cancel_start, cancel_end) = if has_pagination {
                    // Next button at 2-16, Execute after Next, Cancel after Execute
                    (Some((2u16, 17u16)), 18u16, 36u16, 37u16, 54u16)
                } else {
                    // No Next button, Execute starts right after border
                    (None, 2u16, 20u16, 21u16, 38u16)
                };

                (Some(button_screen_y), next_btn, Some((exec_start, exec_end)), Some((cancel_start, cancel_end)))
            } else {
                (None, None, None, None)
            }
        } else {
            (None, None, None, None)
        };

        command_card_hits.push(CommandCardHitArea {
            message_idx: msg_idx,
            start_y: screen_start_y,
            end_y: screen_end_y,
            button_y,
            next_btn,
            execute_btn,
            cancel_btn,
        });
    }

    *assistant.cached_command_cards.borrow_mut() = command_card_hits;
}

/// Render a command suggestion card
/// `pagination` is Some((current, total)) for multi-command display, None for single command or history
fn render_command_card(
    command: &str,
    explanation: &str,
    status: CommandStatus,
    verdict: &Verdict,
    width: u16,
    pagination: Option<(usize, usize)>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Determine verdict label and style based on verdict
    let (verdict_label, verdict_style) = match verdict {
        Verdict::Allow => ("✓ Allow", Style::default().fg(Color::Green)),
        Verdict::RequireConfirmation(_) => ("⚠ Confirm", Style::default().fg(Color::Yellow)),
        Verdict::Deny(_) => ("✗ Deny", Style::default().fg(Color::Red).bold()),
    };

    // Determine border color based on status (overrides verdict color when not pending)
    let border_color = match status {
        CommandStatus::Executed => Color::Cyan,
        CommandStatus::Rejected => Color::Red,
        CommandStatus::Pending => match verdict {
            Verdict::Allow => Color::Green,
            Verdict::RequireConfirmation(_) => Color::Yellow,
            Verdict::Deny(_) => Color::Red,
        },
    };

    let card_width = (width as usize).saturating_sub(4).max(20);

    // Top border with pagination indicator
    let title = if let Some((current, total)) = pagination {
        format!(" Suggestion ({}/{}) ", current, total)
    } else {
        String::new()
    };

    let border_style = Style::default().fg(border_color);
    if title.is_empty() {
        let top_border = format!(" ┌{}┐", "─".repeat(card_width));
        lines.push(Line::from(vec![Span::styled(top_border, border_style)]));
    } else {
        // Center the title in the top border
        let title_len = title.chars().count();
        let remaining = card_width.saturating_sub(title_len);
        let left_dashes = remaining / 2;
        let right_dashes = remaining - left_dashes;
        lines.push(Line::from(vec![
            Span::styled(format!(" ┌{}", "─".repeat(left_dashes)), border_style),
            Span::styled(title, border_style),
            Span::styled(format!("{}┐", "─".repeat(right_dashes)), border_style),
        ]));
    }

    // Verdict line with reason
    let verdict_text = if let Some(r) = verdict.reason() {
        format!("{}: {}", verdict_label, r)
    } else {
        verdict_label.to_string()
    };
    let verdict_line = format_card_line(&verdict_text, card_width);
    lines.push(Line::from(vec![
        Span::styled(" │", Style::default().fg(border_color)),
        Span::styled(verdict_line, verdict_style),
        Span::styled("│", Style::default().fg(border_color)),
    ]));

    // Explanation lines (supports multi-line wrapping)
    if !explanation.is_empty() {
        for exp_line in wrap_text(explanation, card_width) {
            let padded = format!("{:width$}", exp_line, width = card_width);
            lines.push(Line::from(vec![
                Span::styled(" │", border_style),
                Span::styled(padded, border_style),
                Span::styled("│", border_style),
            ]));
        }
    }

    // Command line with > prefix
    let cmd_display = format!("> {}", command);
    let cmd_line = format_card_line(&cmd_display, card_width);
    lines.push(Line::from(vec![
        Span::styled(" │", border_style),
        Span::styled(cmd_line, Style::default().fg(Color::White).bold()),
        Span::styled("│", border_style),
    ]));

    // Status/action line based on verdict and status
    match status {
        CommandStatus::Pending => {
            // Button styles with distinct backgrounds
            let next_btn_style = Style::default().fg(Color::White).bg(Color::Blue).bold();
            let exec_btn_style = Style::default().fg(Color::White).bg(Color::Rgb(0, 100, 0)).bold();
            let cancel_btn_style = Style::default().fg(Color::White).bg(Color::Rgb(139, 0, 0)).bold();

            // Determine button labels based on verdict
            let exec_label = if verdict.is_deny() { " [Ctrl+Y] Copy " } else { " [Ctrl+Y] Execute " };

            let mut spans = vec![Span::styled(" │", border_style)];

            // Add Next button if pagination exists
            if pagination.is_some() {
                spans.push(Span::styled(" [Ctrl+A] Next ", next_btn_style));
                spans.push(Span::raw(" "));
            }

            // Execute/Copy button
            spans.push(Span::styled(exec_label, exec_btn_style));
            spans.push(Span::raw(" "));

            // Cancel button
            spans.push(Span::styled(" [Ctrl+N] Cancel ", cancel_btn_style));

            // Calculate padding to fill card width
            let btn_content_width: usize = spans.iter().skip(1).map(|s| s.content.chars().count()).sum();
            let padding_needed = card_width.saturating_sub(btn_content_width);
            if padding_needed > 0 {
                spans.push(Span::styled(" ".repeat(padding_needed), border_style));
            }

            spans.push(Span::styled("│", border_style));
            lines.push(Line::from(spans));
        }
        CommandStatus::Executed => {
            let status_text = if verdict.is_deny() { "✓ Copied" } else { "✓ Executed" };
            let status_line = format_card_line(status_text, card_width);
            lines.push(Line::from(vec![
                Span::styled(" │", border_style),
                Span::styled(status_line, Style::default().fg(Color::Cyan)),
                Span::styled("│", border_style),
            ]));
        }
        CommandStatus::Rejected => {
            let status_line = format_card_line("✗ Rejected", card_width);
            lines.push(Line::from(vec![
                Span::styled(" │", border_style),
                Span::styled(status_line, Style::default().fg(Color::Red).dim()),
                Span::styled("│", border_style),
            ]));
        }
    }

    // Bottom border
    let bottom_border = format!(" └{}┘", "─".repeat(card_width));
    lines.push(Line::from(vec![Span::styled(bottom_border, border_style)]));

    lines
}

/// Format a line to fit within the card width, padding or truncating as needed
fn format_card_line(text: &str, width: usize) -> String {
    let text_width = text.chars().count();
    if text_width >= width {
        // Truncate with ellipsis
        let truncated: String = text.chars().take(width.saturating_sub(3)).collect();
        format!("{}...", truncated)
    } else {
        // Pad with spaces
        format!("{:width$}", text, width = width)
    }
}

/// Wrap text into multiple lines based on width (word-aware wrapping)
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut result = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_len = word.chars().count();

        if current_width == 0 {
            // First word on line
            if word_len > width {
                // Word itself is too long, force split
                let mut chars = word.chars().peekable();
                while chars.peek().is_some() {
                    let chunk: String = chars.by_ref().take(width).collect();
                    result.push(chunk);
                }
            } else {
                current_line = word.to_string();
                current_width = word_len;
            }
        } else if current_width + 1 + word_len <= width {
            // Word fits on current line with space
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_len;
        } else {
            // Start new line
            result.push(current_line);
            if word_len > width {
                // Word itself is too long, force split
                let mut chars = word.chars().peekable();
                while chars.peek().is_some() {
                    let chunk: String = chars.by_ref().take(width).collect();
                    result.push(chunk);
                }
                current_line = String::new();
                current_width = 0;
            } else {
                current_line = word.to_string();
                current_width = word_len;
            }
        }
    }

    // Don't forget the last line
    if !current_line.is_empty() {
        result.push(current_line);
    }

    if result.is_empty() {
        result.push(String::new());
    }

    result
}

/// Render the input box at the bottom with multi-line support and selection highlighting
fn render_input_box(assistant: &TuiAssistant, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Cache the input area width for cursor movement calculations
    assistant.last_input_area_width.set(inner.width);

    // Get selection range (if any)
    let selection_range = assistant.get_input_selection_range();

    // Render input prompt and text directly to buffer for selection support
    let prompt = assistant.prompt();
    let prompt_width = assistant.prompt_width();
    let input_text = assistant.get_input();

    let prompt_style = Style::default().fg(Color::Cyan);
    let normal_style = Style::default();
    let selection_style = Style::default().fg(Color::White).bg(Color::Blue);

    // Render prompt
    let mut x = inner.x;
    let mut y = inner.y;
    for ch in prompt.chars() {
        if x >= inner.x + inner.width {
            break;
        }
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(ch).set_style(prompt_style);
        }
        x += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
    }

    // Track byte position for selection checking
    let mut byte_pos: usize = 0;
    let mut current_x = prompt_width;

    // Process each character
    for ch in input_text.chars() {
        let char_len = ch.len_utf8();

        if ch == '\n' {
            // Manual newline: move to next line
            y += 1;
            current_x = 0;
            x = inner.x;
            byte_pos += char_len;
            continue;
        }

        let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;

        // Check if we need to wrap
        if current_x + char_width > inner.width {
            // Wrap to next line
            y += 1;
            current_x = 0;
            x = inner.x;
        }

        // Check if we're still within the visible area
        if y >= inner.y + inner.height {
            break;
        }

        // Determine style based on selection
        let is_selected = selection_range.map_or(false, |(start, end)| {
            byte_pos >= start && byte_pos < end
        });
        let style = if is_selected { selection_style } else { normal_style };

        // Render character
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(ch).set_style(style);
        }

        x += char_width;
        current_x += char_width;
        byte_pos += char_len;
    }
}

/// Find word boundaries at the given character position in a string.
/// Returns (start_col, end_col) of the word.
fn find_word_boundaries_in_string(text: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    if len == 0 || col >= len {
        return (col, col);
    }

    let is_word_char = |c: char| -> bool {
        c.is_alphanumeric() || c == '_' || c == '-'
    };

    let current_char = chars[col];

    // If clicking on whitespace, just select that cell
    if current_char.is_whitespace() {
        return (col, col);
    }

    // Find word start
    let mut start = col;
    while start > 0 {
        let prev_char = chars[start - 1];
        if !is_word_char(prev_char) {
            break;
        }
        start -= 1;
    }

    // Find word end
    let mut end = col;
    while end < len.saturating_sub(1) {
        let next_char = chars[end + 1];
        if !is_word_char(next_char) {
            break;
        }
        end += 1;
    }

    (start, end)
}
