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

use crate::event::AppEvent;

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
struct TerminalEventListener;

impl TerminalEventListener {
    fn new() -> Self {
        Self
    }
}

impl EventListener for TerminalEventListener {
    fn send_event(&self, _event: Event) {
        // Ignore all events
    }
}

/// Terminal display widget that owns the alacritty terminal emulator.
pub struct TuiTerminal {
    term: Term<TerminalEventListener>,
    processor: Processor,
    pty_output: Receiver<Vec<u8>>,
    _event_sink: UnboundedSender<AppEvent>,
    scroll_offset: usize,
    error_message: Option<String>,
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

        let event_listener = TerminalEventListener::new();
        let config = Config::default();
        let size = TermSize::new(cols, rows);
        let term = Term::new(config, &size, event_listener);

        Self {
            term,
            processor: Processor::new(),
            pty_output,
            _event_sink: event_sink,
            scroll_offset: 0,
            error_message: None,
        }
    }

    /// Receives and processes PTY output.
    /// Call this in tokio::select! to handle async PTY data.
    pub async fn recv_pty_output(&mut self) {
        if let Some(bytes) = self.pty_output.recv().await {
            self.process(&bytes);
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
            let actual_line_idx = if self.scroll_offset > 0 {
                -((self.scroll_offset - line_idx) as i32)
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
        
        // Render lines directly to buffer
        for (row, line) in lines.iter().enumerate() {
            if row >= area.height as usize {
                break;
            }
            
            let mut x = area.x;
            for span in &line.spans {
                let content = &span.content;
                for c in content.chars() {
                    if x >= area.x + area.width {
                        break;
                    }
                    if let Some(cell) = buf.cell_mut((x, area.y + row as u16)) {
                        cell.set_char(c).set_style(span.style);
                    }
                    x += 1;
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
