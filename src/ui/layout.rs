//! Layout management for the TUI interface.
//!
//! This module defines the layout structure of the application, splitting
//! the screen into different sections (shell panel and AI panel) with
//! configurable sizing and constraints.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct Layouts;

impl Layouts {
    pub fn main_chunks(area: Rect) -> [Rect; 2] {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        [chunks[0], chunks[1]]
    }
}
/// Calculate the actual shell area size based on terminal dimensions
/// This accounts for:
/// - Top and bottom borders (2 rows)
/// - Right sidebar width (40 columns)
pub fn calculate_shell_size(terminal_cols: u16, terminal_rows: u16) -> (u16, u16) {
    const SIDEBAR_WIDTH: u16 = 40;
    const BORDER_HEIGHT: u16 = 2; // Top + Bottom borders

    // Calculate available columns (subtract sidebar width)
    let cols = if terminal_cols > SIDEBAR_WIDTH {
        terminal_cols - SIDEBAR_WIDTH
    } else {
        1 // Minimum 1 column
    };

    // Calculate available rows (subtract borders)
    let rows = if terminal_rows > BORDER_HEIGHT {
        terminal_rows - BORDER_HEIGHT
    } else {
        1 // Minimum 1 row
    };

    (cols, rows)
}
