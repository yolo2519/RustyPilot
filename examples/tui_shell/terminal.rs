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
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Simple terminal size implementation that satisfies the alacritty Dimensions trait.
#[derive(Debug, Copy, Clone)]
struct TermSize {
    cols: usize,
    rows: usize,
}

impl TermSize {
    /// Creates a new terminal size.
    ///
    /// # Arguments
    /// * `cols` - Width in columns
    /// * `rows` - Height in rows
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
    /// Creates a new event listener.
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
/// This is responsible for parsing VT sequences and rendering terminal output.
pub struct TerminalDisplay {
    term: Term<TerminalEventListener>,
    processor: Processor,
    scroll_offset: usize,  // Number of lines scrolled back (0 = at bottom/latest)
}

impl TerminalDisplay {
    /// Creates a new TerminalDisplay with the specified dimensions.
    pub fn new(cols: u16, rows: u16) -> Self {
        let event_listener = TerminalEventListener::new();
        let config = Config::default();

        // Create the term with TermSize
        let size = TermSize::new(cols, rows);
        let term = Term::new(config, &size, event_listener);

        Self {
            term,
            processor: Processor::new(),
            scroll_offset: 0,
        }
    }

    /// Scroll up by n lines (into history)
    pub fn scroll_up(&mut self, n: usize) {
        let grid = self.term.grid();
        let history_size = grid.history_size();

        // Limit scroll to available history
        self.scroll_offset = (self.scroll_offset + n).min(history_size);
    }

    /// Scroll down by n lines (toward latest)
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Reset scroll to bottom (latest output)
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Check if we're scrolled back in history
    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset > 0
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Process VT100 output data (should be filtered, without terminal queries).
    pub fn process(&mut self, data: &[u8]) {
        // Process data through the VTE processor
        // Note: advance() processes the entire byte slice at once (more efficient)
        self.processor.advance(&mut self.term, data);

        // Auto-scroll to bottom when new data arrives ONLY if not currently scrolled
        // This preserves user's scroll position when reviewing history
        if self.scroll_offset == 0 {
            // At bottom: stay at bottom when new data arrives
            self.scroll_to_bottom();
        }
        // If scrolled back: DON'T auto-scroll, let user review history
    }

    /// Get the current cursor position (row, col).
    pub fn cursor_position(&self) -> (u16, u16) {
        let cursor = self.term.grid().cursor.point;
        // Note: cursor.line can be negative for scrollback, but we return 0-based visible position
        let row = cursor.line.0.max(0) as u16;
        let col = cursor.column.0 as u16;
        (row, col)
    }

    /// Resize the terminal display.
    /// Unlike vt100, this preserves scrollback history via intelligent reflow!
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let size = TermSize::new(cols, rows);
        self.term.resize(&size);
    }

    /// Get rendered lines for display.
    pub fn get_lines(&self) -> Vec<Line<'_>> {
        let grid = self.term.grid();

        let mut lines = Vec::new();

        // When scroll_offset = 0, we show the latest screen_lines() lines (normal view)
        // When scroll_offset > 0, we show older lines from history
        let screen_lines = grid.screen_lines();

        // Iterate through visible lines, considering scroll offset
        for line_idx in 0..screen_lines {
            // Calculate actual line position in the grid
            // Alacritty uses Line(0) as the top visible line
            // Negative Line values access scrollback history
            let actual_line_idx = if self.scroll_offset > 0 {
                // When scrolled back, we need to offset into history
                // Line(-1) is the first line in scrollback history above the viewport
                -((self.scroll_offset - line_idx) as i32)
            } else {
                // At bottom: read current visible lines normally
                line_idx as i32
            };

            let line = TermLine(actual_line_idx);
            let mut spans = Vec::new();
            let mut current_text = String::new();
            let mut current_style = Style::default();

            // Iterate through columns in this line
            for col in 0..grid.columns() {
                let cell = &grid[line][Column(col)];

                // Skip wide character spacers (alacritty marks the second cell of wide chars)
                if cell.flags.contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER) {
                    continue;
                }

                // Get cell content
                let c = cell.c;

                // Build style for this cell
                let mut style = Style::default();

                // Convert colors
                style = style.fg(convert_ansi_color(cell.fg));
                style = style.bg(convert_ansi_color(cell.bg));

                // Add modifiers based on cell flags
                if cell.flags.contains(alacritty_terminal::term::cell::Flags::BOLD) {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.flags.contains(alacritty_terminal::term::cell::Flags::ITALIC) {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.flags.contains(alacritty_terminal::term::cell::Flags::UNDERLINE) {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.flags.contains(alacritty_terminal::term::cell::Flags::INVERSE) {
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

/// Converts alacritty's ANSI color to ratatui color.
///
/// # Arguments
/// * `color` - The alacritty ANSI color to convert
///
/// # Returns
/// The corresponding ratatui Color
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
