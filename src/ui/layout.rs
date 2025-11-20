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
