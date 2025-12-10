//! Terminal display widget that owns the terminal emulator and handles rendering.
//!
//! This module uses alacritty_terminal for robust terminal emulation with support
//! for proper resize handling (with scrollback preservation via reflow).

use alacritty_terminal::term::{Term, Config};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line as TermLine};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Processor};
use ratatui::{
    prelude::{Buffer, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use tokio::sync::mpsc::{Receiver, UnboundedSender};
use tracing::error;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::event::AppEvent;
use super::visual::{VisualState, SelectionMode, PaneStatus, KeyHandleResult, copy_to_clipboard, is_in_selection_with_mode};

/// Simple terminal size implementation that satisfies the alacritty Dimensions trait.
#[derive(Debug, Copy, Clone)]
struct TermSize {
    cols: usize,
    rows: usize,
}

impl TermSize {
    /// Creates a new terminal size.
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols as usize,
            rows: rows as usize,
        }
    }
}

impl Dimensions for TermSize {
    fn columns(&self) -> usize {
        self.cols
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn total_lines(&self) -> usize {
        self.rows
    }
}

impl Dimensions for &TermSize {
    fn columns(&self) -> usize {
        self.cols
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn total_lines(&self) -> usize {
        self.rows
    }
}

/// Minimal event listener that ignores all terminal events.
#[derive(Clone)]
struct TerminalEventListener {
    app_event_sink: UnboundedSender<AppEvent>,
}

impl TerminalEventListener {
    fn new(app_event_sink: UnboundedSender<AppEvent>) -> Self {
        Self {
            app_event_sink,
        }
    }
}

impl EventListener for TerminalEventListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(s) => {
                if let Err(e) = self.app_event_sink.send(AppEvent::PtyWrite(s.into_bytes())) {
                    error!("Failed to send PtyWrite event: {:?}", e);
                    // Note: This error is very unlikely to occur as it would mean
                    // the event receiver has been dropped while the terminal is still active.
                }
            }
            _ => {}
        }
    }
}

/// Terminal display widget that owns the alacritty terminal emulator.
pub struct TuiTerminal {
    term: Term<TerminalEventListener>,
    processor: Processor,
    pty_output: Receiver<Vec<u8>>,
    event_sink: UnboundedSender<AppEvent>,
    scroll_offset: usize,
    error_message: Option<String>,

    // Visual mode state
    visual_state: Option<VisualState>,

}

impl TuiTerminal {
    /// Creates a new terminal display.
    pub fn new(
        pty_output: Receiver<Vec<u8>>,
        event_sink: UnboundedSender<AppEvent>,
    ) -> Self {
        // Start with reasonable default size (will be resized on first draw)
        let cols = 80;
        let rows = 24;

        let event_listener = TerminalEventListener::new(event_sink.clone());
        let config = Config::default();
        let size = TermSize::new(cols, rows);
        let term = Term::new(config, &size, event_listener);

        Self {
            term,
            processor: Processor::new(),
            pty_output,
            event_sink,
            scroll_offset: 0,
            error_message: None,
            visual_state: None,
        }
    }

    /// Receives and processes PTY output.
    /// Call this in tokio::select! to handle async PTY data.
    pub async fn recv_pty_output(&mut self) {
        if let Some(bytes) = self.pty_output.recv().await {
            // Always process PTY output for terminal display (including newlines, etc.)
            self.process(&bytes);

            // TODO: the raw pty output is sometimes just GIBBERISH for AI.
            // TODO: Use rendered output instead.
            // TODO: do shell integration to see the boundary of commands (OSC)
            // Emit a small text snippet for context building (skip pure whitespace)
            let snippet = String::from_utf8_lossy(&bytes);
            let trimmed = snippet.trim();
            if !trimmed.is_empty() {
                // Limit to avoid flooding the event channel
                let truncated: String = trimmed.chars().take(400).collect();
                if let Err(e) = self
                    .event_sink
                    .send(AppEvent::ShellOutput { data: truncated })
                {
                    error!("Failed to send shell output event: {:?}", e);
                }
            }
        }
    }

    /// Process VT100 output data.
    fn process(&mut self, data: &[u8]) {
        self.processor.advance(&mut self.term, data);

        // Auto-scroll to bottom when new data arrives ONLY if not scrolled
        if self.scroll_offset == 0 {
            self.scroll_to_bottom();
        }
    }

    /// Scroll up by n lines (into history).
    pub fn scroll_up(&mut self, n: usize) {
        let grid = self.term.grid();
        let history_size = grid.history_size();
        self.scroll_offset = (self.scroll_offset + n).min(history_size);
    }

    /// Scroll down by n lines (toward latest).
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Reset scroll to bottom (latest output).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Check if we're scrolled back in history.
    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset > 0
    }

    /// Get current scroll offset.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Get the current cursor position (row, col).
    pub fn cursor_position(&self) -> (u16, u16) {
        let cursor = self.term.grid().cursor.point;
        let row = cursor.line.0.max(0) as u16;
        let col = cursor.column.0 as u16;
        (row, col)
    }

    /// Resize the terminal display.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let size = TermSize::new(cols, rows);
        self.term.resize(&size);
    }

    /// Display an error message in the terminal.
    pub fn show_error(&mut self, message: &str) {
        self.error_message = Some(message.to_string());
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
    /// Initializes the cursor at the terminal's physical cursor position.
    pub fn enter_visual_mode(&mut self) {
        let grid = self.term.grid();
        let history_size = grid.history_size();

        // Get the terminal's physical cursor position
        let cursor = grid.cursor.point;
        // cursor.line is in screen coordinates (0 = top of screen, can be negative for history)
        // Convert to content row (history_size + screen_row)
        let screen_row = cursor.line.0.max(0) as usize;
        let col = cursor.column.0 as usize;

        // Content row = history_size + screen_row (when scroll_offset = 0)
        // If we're scrolled, the physical cursor is still at the same content position
        let content_row = history_size + screen_row;

        self.visual_state = Some(VisualState::new(content_row, col));

        // Auto-scroll to make sure the cursor is visible
        self.scroll_to_visual_cursor();
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

        let grid = self.term.grid();
        let history_size = grid.history_size();
        let screen_lines = grid.screen_lines();
        let columns = grid.columns();
        let total_lines = history_size + screen_lines;

        // Calculate max row and col
        let max_row = total_lines.saturating_sub(1);
        let max_col = columns.saturating_sub(1);

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

        let grid = self.term.grid();
        let history_size = grid.history_size();
        let screen_lines = grid.screen_lines();
        let total_lines = history_size + screen_lines;

        let (cursor_row, _) = visual.cursor;

        // Calculate the visible range in content coordinates
        // visible_top and visible_bottom are inclusive
        let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
        let visible_top = visible_bottom.saturating_sub(screen_lines.saturating_sub(1));

        // Adjust scroll if cursor is out of visible range
        if cursor_row < visible_top {
            // Cursor is above visible area, scroll up
            self.scroll_offset = total_lines.saturating_sub(1).saturating_sub(cursor_row).saturating_sub(screen_lines.saturating_sub(1));
            self.scroll_offset = self.scroll_offset.min(history_size);
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

        // Extract text from the terminal grid
        let text = self.get_text_range(start_row, start_col, end_row, end_col, mode);
        if text.is_empty() {
            return false;
        }

        copy_to_clipboard(&text)
    }

    /// Get effective width of a line (position after last non-space character).
    /// Returns 0 for empty lines.
    fn get_line_effective_width(&self, content_row: usize) -> usize {
        let grid = self.term.grid();
        let history_size = grid.history_size();
        let columns = grid.columns();

        let grid_line_idx = content_row as i32 - history_size as i32;
        let line = TermLine(grid_line_idx);

        // Scan from right to find last non-space character
        for col in (0..columns).rev() {
            let cell = &grid[line][Column(col)];
            if cell.c != ' ' && cell.c != '\0' {
                return col + 1;
            }
        }
        0 // Empty line
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
                self.scroll_up(repeat);
            }
            KeyCode::Down if shift => {
                let repeat = visual.take_repeat_count();
                self.scroll_down(repeat);
            }
            KeyCode::PageUp => {
                let repeat = visual.take_repeat_count();
                self.scroll_up(repeat * 10);
            }
            KeyCode::PageDown => {
                let repeat = visual.take_repeat_count();
                self.scroll_down(repeat * 10);
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

    /// Get text from a range in content coordinates.
    /// Get text from a range in content coordinates.
    /// Line mode: trims trailing spaces from each line, adjusts col range per line.
    /// Block mode: extracts exact rectangle, preserves spaces.
    fn get_text_range(&self, start_row: usize, start_col: usize, end_row: usize, end_col: usize, mode: SelectionMode) -> String {
        let grid = self.term.grid();
        let history_size = grid.history_size();
        let columns = grid.columns();

        let mut result = String::new();

        for row in start_row..=end_row {
            // Convert content row to grid line index
            let grid_line_idx = row as i32 - history_size as i32;
            let line = TermLine(grid_line_idx);

            // Get effective line width for Line mode clamping
            let line_width = self.get_line_effective_width(row);

            // Determine column range based on mode
            let (col_start, col_end) = match mode {
                SelectionMode::None => continue,
                SelectionMode::Line => {
                    // Line mode: clamp to effective line width
                    if line_width == 0 {
                        // Empty line, skip content but keep newline
                        if row < end_row {
                            result.push('\n');
                        }
                        continue;
                    }
                    let cs = if row == start_row { start_col.min(line_width.saturating_sub(1)) } else { 0 };
                    let ce = if row == end_row {
                        end_col.min(line_width.saturating_sub(1))
                    } else {
                        line_width.saturating_sub(1)
                    };
                    (cs, ce)
                }
                SelectionMode::Block => {
                    // Block mode: use exact rectangle columns
                    (start_col, end_col.min(columns.saturating_sub(1)))
                }
            };

            // Extract characters from this line
            let mut line_text = String::new();
            for col in col_start..=col_end {
                if col >= columns {
                    break;
                }
                let cell = &grid[line][Column(col)];

                // Skip wide character spacers
                if cell.flags.contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER) {
                    continue;
                }

                line_text.push(cell.c);
            }

            // For Line mode, trim trailing whitespace
            // For Block mode, preserve as-is
            let final_text = match mode {
                SelectionMode::Line => line_text.trim_end().to_string(),
                SelectionMode::Block | SelectionMode::None => line_text,
            };
            result.push_str(&final_text);

            // Add newline between lines (but not after the last line)
            if row < end_row {
                result.push('\n');
            }
        }

        result
    }

    /// Get visual cursor position in screen coordinates (for rendering).
    /// Returns (screen_row, screen_col) if cursor is visible, None otherwise.
    pub fn get_visual_cursor_screen_pos(&self) -> Option<(usize, usize)> {
        let visual = self.visual_state.as_ref()?;
        let (cursor_row, cursor_col) = visual.cursor;

        let grid = self.term.grid();
        let history_size = grid.history_size();
        let screen_lines = grid.screen_lines();
        let total_lines = history_size + screen_lines;

        // Calculate visible range
        let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
        let visible_top = visible_bottom.saturating_sub(screen_lines.saturating_sub(1));

        // Check if cursor is in visible range
        if cursor_row >= visible_top && cursor_row <= visible_bottom {
            let screen_row = cursor_row - visible_top;
            Some((screen_row, cursor_col))
        } else {
            None
        }
    }

    /// Convert screen position to content row for selection checking.
    fn screen_row_to_content_row(&self, screen_row: usize) -> usize {
        let grid = self.term.grid();
        let history_size = grid.history_size();
        let screen_lines = grid.screen_lines();
        let total_lines = history_size + screen_lines;

        let visible_bottom = total_lines.saturating_sub(1).saturating_sub(self.scroll_offset);
        let visible_top = visible_bottom.saturating_sub(screen_lines.saturating_sub(1));

        visible_top + screen_row
    }

    /// Get rendered lines for display.
    fn get_lines(&self) -> Vec<Line<'_>> {
        let grid = self.term.grid();
        let mut lines = Vec::new();

        // Show error message if present
        if let Some(ref err) = self.error_message {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("[ERROR] {}", err),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
        }

        let screen_lines = grid.screen_lines();

        // Iterate through visible lines, considering scroll offset
        for line_idx in 0..screen_lines {
            // When scrolled up, we want to show lines from history
            // line_idx 0 at scroll_offset N should show line -N
            let actual_line_idx = if self.scroll_offset > 0 {
                line_idx as i32 - self.scroll_offset as i32
            } else {
                line_idx as i32
            };

            let line = TermLine(actual_line_idx);
            let mut spans = Vec::new();
            let mut current_text = String::new();
            let mut current_style = Style::default();

            // Iterate through columns in this line
            for col in 0..grid.columns() {
                let cell = &grid[line][Column(col)];

                // Skip wide character spacers
                if cell
                    .flags
                    .contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER)
                {
                    continue;
                }

                let c = cell.c;

                // Build style for this cell
                let mut style = Style::default();
                style = style.fg(convert_ansi_color(cell.fg));
                style = style.bg(convert_ansi_color(cell.bg));

                if cell
                    .flags
                    .contains(alacritty_terminal::term::cell::Flags::BOLD)
                {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell
                    .flags
                    .contains(alacritty_terminal::term::cell::Flags::ITALIC)
                {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell
                    .flags
                    .contains(alacritty_terminal::term::cell::Flags::UNDERLINE)
                {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell
                    .flags
                    .contains(alacritty_terminal::term::cell::Flags::INVERSE)
                {
                    style = style.add_modifier(Modifier::REVERSED);
                }

                // Group consecutive cells with the same style
                if style == current_style {
                    current_text.push(c);
                } else {
                    if !current_text.is_empty() {
                        spans.push(Span::styled(current_text.clone(), current_style));
                        current_text.clear();
                    }
                    current_style = style;
                    current_text.push(c);
                }
            }

            // Add the last span
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text, current_style));
            }

            // If no spans, add empty line to maintain layout
            if spans.is_empty() {
                spans.push(Span::raw(""));
            }

            lines.push(Line::from(spans));
        }

        lines
    }
}

impl Widget for &TuiTerminal {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Resize terminal if needed
        let grid = self.term.grid();
        if grid.columns() != area.width as usize || grid.screen_lines() != area.height as usize {
            // Note: We can't mutate self in render(), so resize should be done elsewhere
            // This is just for display - actual resize happens in app event loop
        }

        let lines = self.get_lines();

        // Get visual mode state for highlighting
        let visual_cursor_pos = self.get_visual_cursor_screen_pos();
        let selection_range = self.visual_state.as_ref().and_then(|v| v.selection_range());
        let selection_mode = self.visual_state.as_ref().map(|v| v.get_selection_mode()).unwrap_or(SelectionMode::None);

        // Render lines directly to buffer
        for (row, line) in lines.iter().enumerate() {
            if row >= area.height as usize {
                break;
            }

            let mut x = area.x;
            let mut col: usize = 0;

            for span in &line.spans {
                let content = &span.content;
                for c in content.chars() {
                    if x >= area.x + area.width {
                        break;
                    }

                    // Render tab as a single space since alacritty_terminal has already
                    // handled cursor movement. The tab character in the grid just marks
                    // the position where tab was, subsequent characters are already at
                    // correct positions.
                    let render_char = if c == '\t' { ' ' } else { c };

                    // Determine style with visual mode modifications
                    let mut style = span.style;

                    // Check if this cell is the visual cursor
                    let is_cursor = visual_cursor_pos.map_or(false, |(cr, cc)| cr == row && cc == col);

                    // Check if this cell is in the selection range
                    let is_selected = selection_range.map_or(false, |(start, end)| {
                        let content_row = self.screen_row_to_content_row(row);
                        is_in_selection_with_mode(
                            content_row,
                            col,
                            start,
                            end,
                            selection_mode,
                            |r| self.get_line_effective_width(r),
                        )
                    });

                    if is_cursor {
                        // Visual cursor: blue background, white foreground
                        style = Style::default().fg(Color::White).bg(Color::Blue);
                    } else if is_selected {
                        // Selection: blue background, white foreground
                        style = Style::default().fg(Color::White).bg(Color::Blue);
                    }

                    if let Some(cell) = buf.cell_mut((x, area.y + row as u16)) {
                        cell.set_char(render_char).set_style(style);
                    }
                    x += 1;
                    col += 1;
                }
            }

            // If cursor is on this row but beyond the rendered content, render it
            if let Some((cursor_row, cursor_col)) = visual_cursor_pos {
                if cursor_row == row && cursor_col >= col {
                    let cursor_x = area.x + cursor_col as u16;
                    if cursor_x < area.x + area.width {
                        let style = Style::default().fg(Color::White).bg(Color::Blue);
                        if let Some(cell) = buf.cell_mut((cursor_x, area.y + row as u16)) {
                            cell.set_char(' ').set_style(style);
                        }
                    }
                }
            }
        }
    }
}

/// Converts alacritty's ANSI color to ratatui color.
fn convert_ansi_color(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Named(named) => match named {
            NamedColor::Black => Color::Black,
            NamedColor::Red => Color::Red,
            NamedColor::Green => Color::Green,
            NamedColor::Yellow => Color::Yellow,
            NamedColor::Blue => Color::Blue,
            NamedColor::Magenta => Color::Magenta,
            NamedColor::Cyan => Color::Cyan,
            NamedColor::White => Color::White,
            NamedColor::BrightBlack => Color::DarkGray,
            NamedColor::BrightRed => Color::LightRed,
            NamedColor::BrightGreen => Color::LightGreen,
            NamedColor::BrightYellow => Color::LightYellow,
            NamedColor::BrightBlue => Color::LightBlue,
            NamedColor::BrightMagenta => Color::LightMagenta,
            NamedColor::BrightCyan => Color::LightCyan,
            NamedColor::BrightWhite => Color::Gray,
            NamedColor::Foreground => Color::Reset,
            NamedColor::Background => Color::Reset,
            _ => Color::Reset,
        },
        AnsiColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(idx) => Color::Indexed(idx),
    }
}
