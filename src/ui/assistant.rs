//! TUI Assistant sidebar component for AI chat interactions.
//!
//! This module provides a sidebar widget that supports:
//! - Multiple chat sessions with tab switching
//! - Chat message display (user messages, AI responses, command cards)
//! - Streaming AI response rendering
//! - Command suggestion cards with execute/cancel actions
//! - Text input with cursor support

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::Buffer;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use tokio::sync::mpsc::Receiver;

use crate::ai::session::SessionId;
use crate::event::AiStreamData;

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

/// The main AI Assistant sidebar widget
pub struct TuiAssistant {
    // AI stream receiver (dedicated channel for high-frequency streaming data)
    ai_stream: Receiver<AiStreamData>,

    // Session management
    session_tabs: Vec<SessionTab>,
    active_session: SessionId,

    // Current session's messages (UI rendering format)
    messages: Vec<ChatMessage>,

    // Input state
    input_buffer: String,
    input_cursor: usize,

    // Scroll state
    scroll_offset: u16,

    // Pending command (index into messages vec)
    pending_command_idx: Option<usize>,

    // ID counter for new sessions
    next_session_id: SessionId,
}

impl TuiAssistant {
    pub fn new(ai_stream: Receiver<AiStreamData>) -> Self {
        let initial_session = SessionTab {
            id: 1,
            name: "Session 1".to_string(),
        };
        Self {
            ai_stream,
            session_tabs: vec![initial_session],
            active_session: 1,
            messages: Vec::new(),
            input_buffer: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            pending_command_idx: None,
            next_session_id: 2,
        }
    }

    /// Await on the AI stream and process incoming data.
    /// Call this in a tokio::select! branch.
    pub async fn recv_ai_stream(
        &mut self,
        ai_sessions: &mut crate::ai::session::AiSessionManager,
    ) -> Option<()> {
        let data = self.ai_stream.recv().await?;
        match data {
            AiStreamData::Chunk { session_id, text } => {
                ai_sessions.append_chunk(session_id, &text);
                if session_id == self.active_session {
                    self.append_stream_chunk(&text);
                }
            }
            AiStreamData::End { session_id } => {
                if let Some(current) = ai_sessions
                    .get_current_response(session_id)
                    .map(|s| s.to_string())
                {
                    ai_sessions.finalize_response(session_id, current);
                }
                if session_id == self.active_session {
                    self.end_stream();
                }
            }
            AiStreamData::Error { session_id, error } => {
                if session_id == self.active_session {
                    self.end_stream();
                    self.push_user_message(format!("[Error] {}", error));
                }
            }
        }
        Some(())
    }
}

// Note: No Default impl because TuiAssistant requires an ai_stream receiver

impl TuiAssistant {
    // ========================================================================
    // Session Management
    // ========================================================================

    /// Switch to a different session by ID
    pub fn switch_session(&mut self, id: SessionId) {
        if self.session_tabs.iter().any(|t| t.id == id) {
            self.active_session = id;
            // TODO: Load messages for this session from backend
            // For now, clear messages when switching (backend integration pending)
            self.messages.clear();
            self.scroll_offset = 0;
            self.pending_command_idx = None;
        }
    }

    /// Add a new session and switch to it
    pub fn add_session(&mut self, name: String) -> SessionId {
        let id = self.next_session_id;
        self.next_session_id += 1;
        self.session_tabs.push(SessionTab { id, name });
        self.switch_session(id);
        id
    }

    /// Get the current active session ID
    pub fn active_session_id(&self) -> SessionId {
        self.active_session
    }

    /// Get all session tabs
    pub fn session_tabs(&self) -> &[SessionTab] {
        &self.session_tabs
    }

    /// Switch to the next session (cycles through tabs)
    pub fn next_session(&mut self) {
        if let Some(idx) = self.session_tabs.iter().position(|t| t.id == self.active_session) {
            let next_idx = (idx + 1) % self.session_tabs.len();
            self.switch_session(self.session_tabs[next_idx].id);
        }
    }

    /// Switch to the previous session (cycles through tabs)
    pub fn prev_session(&mut self) {
        if let Some(idx) = self.session_tabs.iter().position(|t| t.id == self.active_session) {
            let prev_idx = if idx == 0 {
                self.session_tabs.len() - 1
            } else {
                idx - 1
            };
            self.switch_session(self.session_tabs[prev_idx].id);
        }
    }

    // ========================================================================
    // Message Management
    // ========================================================================

    /// Add a user message to the conversation
    pub fn push_user_message(&mut self, text: String) {
        self.messages.push(ChatMessage::User { text });
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

    /// Scroll the message list by delta lines (positive = down, negative = up)
    pub fn scroll(&mut self, delta: i16) {
        if delta < 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub((-delta) as u16);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_add(delta as u16);
        }
    }

    /// Scroll to the bottom of the message list
    pub fn scroll_to_bottom(&mut self) {
        // This will be adjusted during rendering based on actual content height
        self.scroll_offset = u16::MAX;
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> u16 {
        self.scroll_offset
    }

    /// Check if we're scrolled back (not at bottom)
    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset != u16::MAX && self.scroll_offset > 0
    }

    /// Calculate the number of lines needed to display the input text.
    /// Returns the number of lines (minimum 1).
    pub fn calculate_input_lines(&self, width: u16) -> u16 {
        if width == 0 {
            return 1;
        }

        let prompt = "> ";
        let prompt_width = prompt.len() as u16;

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

    /// Calculate the actual input box height (including border) for a given area height and width.
    pub fn calculate_input_box_height(&self, area_height: u16, area_width: u16) -> u16 {
        let input_text_lines = self.calculate_input_lines(area_width);
        let min_input_height = 3u16;
        let max_input_height = (area_height / 2).max(min_input_height);
        (input_text_lines + 1).clamp(min_input_height, max_input_height)
    }

    /// Get cursor position for rendering with the given input area dimensions.
    /// Returns Some((x, y)) if cursor should be shown, None otherwise.
    /// Coordinates are relative to the input box inner area.
    pub fn get_cursor_position(&self, input_area_width: u16) -> Option<(u16, u16)> {
        let prompt = "> ";
        let prompt_width = prompt.len() as u16;

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
                // Wrap to next line
                x = 0;
                y += 1;
            }
            x += char_width;
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

/// Render the session tab bar
fn render_tab_bar(assistant: &TuiAssistant, area: Rect, buf: &mut Buffer) {
    let mut spans = Vec::new();

    for (i, tab) in assistant.session_tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }

        let is_active = tab.id == assistant.active_session;
        let style = if is_active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        let tab_text = format!(" {} ", tab.name);
        spans.push(Span::styled(tab_text, style));
    }

    // Add "+" button for new session
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
                all_lines.push(Line::raw("")); // Empty line after message
            }
            ChatMessage::CommandCard {
                command,
                explanation,
                status,
            } => {
                all_lines.extend(render_command_card(command, explanation, *status, area.width));
                all_lines.push(Line::raw("")); // Empty line after card
            }
        }
    }

    // Calculate scroll offset
    let total_lines = all_lines.len() as u16;
    let visible_lines = area.height;
    let max_scroll = total_lines.saturating_sub(visible_lines);

    // Adjust scroll_offset if it's MAX (scroll to bottom)
    let effective_scroll = if assistant.scroll_offset == u16::MAX {
        max_scroll
    } else {
        assistant.scroll_offset.min(max_scroll)
    };

    // Skip lines based on scroll offset
    let skip = effective_scroll as usize;
    let visible: Vec<Line> = all_lines.into_iter().skip(skip).take(visible_lines as usize).collect();

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

    // Render input prompt and text
    let prompt = "> ";
    let prompt_width = prompt.len() as u16;
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
