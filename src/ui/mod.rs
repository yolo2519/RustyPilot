//! User interface module for the TUI application.
//!
//! This module contains all UI-related components including terminal initialization,
//! layout management, panel rendering, application state, and the main event loop.

use ratatui::{layout::{Constraint, Direction, Layout}, style::Stylize, text::Line, widgets::{Block, Borders, Widget}};

use crate::app::App;

pub mod assistant;
pub mod terminal;

impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        let top_title_text = match self.get_active_pane() {
            crate::app::ActivePane::Terminal => format!(" RustyTerm [TERMINAL] "),
            crate::app::ActivePane::Assistant => format!(" RustyTerm [ASSISTANT] "),
        };
        let top = Line::from(top_title_text.bold());
        let outer_block = Block::new()
            .borders(Borders::all())
            .title(top);
        let inner_area = outer_block.inner(area);
        outer_block.render(area, buf);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(20),      // Left: shell (takes remaining space)
                Constraint::Length(50),  // Right: sidebar
            ])
            .split(inner_area);
        self.tui_terminal.render(chunks[0], buf);
        self.tui_assistant.render(chunks[1], buf);

    }
}
