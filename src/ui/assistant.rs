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

use crate::ai::session::SessionId;
use crate::event::AiUiUpdate;

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

    // Scroll state (0 = at bottom, >0 = scrolled up by N lines)
    scroll_offset: usize,

    // Pending command (index into messages vec)
    pending_command_idx: Option<usize>,

    // Cached rendering dimensions (updated during render, uses Cell for interior mutability)
    last_input_area_width: Cell<u16>,

    // Cached max scroll offset (updated during render)
    max_scroll_offset: Cell<usize>,
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
            scroll_offset: 0,
            pending_command_idx: None,
            last_input_area_width: Cell::new(80), // Default value
            max_scroll_offset: Cell::new(0),
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
                command,
                explanation,
            } => {
                if session_id == self.active_session {
                    // End the streaming message first
                    self.end_stream();
                    // Then add the command card
                    self.push_command_card(command, explanation);
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

    /// Add a command suggestion card
    pub fn push_command_card(&mut self, command: String, explanation: String) {
        let idx = self.messages.len();
        self.messages.push(ChatMessage::CommandCard {
            command,
            explanation,
            status: CommandStatus::Pending,
        });
        self.pending_command_idx = Some(idx);
        self.scroll_to_bottom();
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

    /// Confirm the pending command (Y key) - returns the command string
    pub fn confirm_command(&mut self) -> Option<String> {
        if let Some(idx) = self.pending_command_idx.take() {
            if let Some(ChatMessage::CommandCard { command, status, .. }) =
                self.messages.get_mut(idx)
            {
                *status = CommandStatus::Executed;
                return Some(command.clone());
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

    if num_tabs == 0 {
        // Just show the "+" button
        spans.push(Span::styled(" + ", Style::default().fg(Color::Green)));
        let line = Line::from(spans);
        Paragraph::new(line).render(area, buf);
        return;
    }

    // Find the index of the active session
    let active_idx = tabs
        .iter()
        .position(|t| t.id == assistant.active_session)
        .unwrap_or(0);

    // Calculate available width for tabs (excluding "+" button)
    let total_width = area.width as usize;
    let available_for_tabs = total_width.saturating_sub(TAB_PLUS_BUTTON_WIDTH);

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
        spans.push(Span::styled(
            format!("‹{} ", hidden_left),
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
        }

        let is_active = tab.id == assistant.active_session;
        let is_next = Some(tab_idx) == next_in_visible && num_tabs > 1;

        // Add Tab hint before the next session (subtle indicator)
        if is_next && !is_active {
            spans.push(Span::styled("⇥", Style::default().fg(Color::DarkGray)));
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
        spans.push(Span::styled(tab_text, style));
    }

    // Render right hidden indicator
    if hidden_right > 0 {
        spans.push(Span::styled(
            format!(" {}›", hidden_right),
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Add "+" button for new session (Ctrl+B,T in command mode)
    spans.push(Span::raw(" "));
    spans.push(Span::styled(" + ", Style::default().fg(Color::Green)));

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

    // Build all lines from messages
    let mut all_lines: Vec<Line> = Vec::new();

    for msg in &assistant.messages {
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
            } => {
                all_lines.extend(render_command_card(command, explanation, *status, area.width));
                all_lines.push(Line::raw("")); // Empty line after card
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

    // Cache max_scroll for scroll limiting
    assistant.max_scroll_offset.set(max_scroll);

    let effective_scroll = assistant.scroll_offset.min(max_scroll);

    // Calculate which lines to show
    // When scroll_offset = 0, we show the last N lines (at bottom)
    // When scroll_offset = max, we show the first N lines (at top)
    let skip = total_lines.saturating_sub(visible_lines + effective_scroll);
    let visible: Vec<Line> = all_lines.into_iter().skip(skip).take(visible_lines).collect();

    // Render without wrap since we already handled wrapping manually
    let paragraph = Paragraph::new(visible);
    paragraph.render(area, buf);
}

/// Render a command suggestion card
fn render_command_card(
    command: &str,
    explanation: &str,
    status: CommandStatus,
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Card border style based on status
    let (border_color, status_text) = match status {
        CommandStatus::Pending => (Color::Yellow, "[Ctrl+Y] Execute  [Ctrl+N] Cancel"),
        CommandStatus::Executed => (Color::Green, "Executed"),
        CommandStatus::Rejected => (Color::Red, "Rejected"),
    };

    let card_width = (width as usize).saturating_sub(4).max(20);

    // Top border
    let top_border = format!(" ┌{}┐", "─".repeat(card_width));
    lines.push(Line::styled(top_border, Style::default().fg(border_color)));

    // Explanation line (if not empty)
    if !explanation.is_empty() {
        let exp_line = format_card_line(explanation, card_width);
        lines.push(Line::styled(
            format!(" │{}│", exp_line),
            Style::default().fg(border_color),
        ));
    }

    // Command line with $ prefix
    let cmd_display = format!("> {}", command);
    let cmd_line = format_card_line(&cmd_display, card_width);
    lines.push(Line::from(vec![
        Span::styled(" │", Style::default().fg(border_color)),
        Span::styled(cmd_line, Style::default().fg(Color::White).bold()),
        Span::styled("│", Style::default().fg(border_color)),
    ]));

    // Status/action line
    let status_line = format_card_line(status_text, card_width);
    let status_style = match status {
        CommandStatus::Pending => Style::default().fg(Color::Yellow),
        CommandStatus::Executed => Style::default().fg(Color::Green),
        CommandStatus::Rejected => Style::default().fg(Color::Red).dim(),
    };
    lines.push(Line::from(vec![
        Span::styled(" │", Style::default().fg(border_color)),
        Span::styled(status_line, status_style),
        Span::styled("│", Style::default().fg(border_color)),
    ]));

    // Bottom border
    let bottom_border = format!(" └{}┘", "─".repeat(card_width));
    lines.push(Line::styled(bottom_border, Style::default().fg(border_color)));

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

/// Render the input box at the bottom with multi-line support
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

    // Render input prompt and text
    let prompt = assistant.prompt();
    let prompt_width = assistant.prompt_width();
    let input_text = assistant.get_input();

    // Build lines with wrapping (no auto-formatting, just hard wrap at width)
    let mut lines: Vec<Line> = Vec::new();
    let mut current_line_spans: Vec<Span> = Vec::new();

    // Add prompt at the beginning
    current_line_spans.push(Span::styled(prompt, Style::default().fg(Color::Cyan)));
    let mut current_x = prompt_width;

    // Process each character
    for ch in input_text.chars() {
        if ch == '\n' {
            // Manual newline: finish current line and start a new one
            lines.push(Line::from(std::mem::take(&mut current_line_spans)));
            current_x = 0;
            continue;
        }

        let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u16;

        // Check if we need to wrap
        if current_x + char_width > inner.width {
            // Finish current line and start a new one
            lines.push(Line::from(std::mem::take(&mut current_line_spans)));
            current_x = 0;
        }

        // Add character
        // Note: We don't render cursor here anymore - it will be a real cursor
        current_line_spans.push(Span::raw(ch.to_string()));
        current_x += char_width;
    }

    // Add remaining spans to the last line
    if !current_line_spans.is_empty() {
        lines.push(Line::from(current_line_spans));
    }

    // Ensure at least one line exists
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(prompt, Style::default().fg(Color::Cyan))));
    }

    // Render the lines
    let paragraph = Paragraph::new(lines);
    paragraph.render(inner, buf);
}
