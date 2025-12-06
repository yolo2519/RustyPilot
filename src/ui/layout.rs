//! Layout management for the TUI application.
//!
//! This module handles the layout calculation and configuration for the split-pane
//! interface between terminal and assistant panes. It separates the layout logic
//! from the main application state.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::{Block, Borders};

/// Layout builder - holds user preferences and configuration for layout calculation
///
/// This is the "how to build" - contains all the constraints and preferences
/// that determine the layout structure. Can be serialized to save user preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutBuilder {
    /// Terminal/Assistant split ratio (0-100, percentage for terminal pane)
    /// This is the user's preference and persists across window resizes
    split_ratio: u16,

    // Future extensions can go here:
    // min_terminal_width: u16,
    // min_assistant_width: u16,
    // separator_draggable: bool,
    // etc.
}

impl LayoutBuilder {
    /// Create a new layout builder with default settings
    pub fn new() -> Self {
        Self {
            split_ratio: 60, // Default: 60% terminal, 40% assistant
        }
    }

    /// Set the split ratio (percentage for terminal pane)
    pub fn with_split_ratio(mut self, ratio: u16) -> Self {
        self.split_ratio = ratio.clamp(10, 90);
        self
    }

    /// Get current split ratio
    pub fn split_ratio(&self) -> u16 {
        self.split_ratio
    }

    /// Build an AppLayout from this configuration and terminal area
    ///
    /// # Arguments
    /// * `area` - The full terminal area to layout within
    pub fn build(&self, area: ratatui::layout::Rect) -> AppLayout {
        // Split into three chunks: terminal, separator, assistant
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(self.split_ratio),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(area);

        // Calculate inner areas (without borders)
        let term_block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM | Borders::LEFT);
        let terminal_inner = term_block.inner(chunks[0]);

        let ai_block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM | Borders::RIGHT);
        let assistant_inner = ai_block.inner(chunks[2]);

        AppLayout {
            full_area: area,
            terminal_area: chunks[0],
            terminal_inner,
            separator_area: chunks[1],
            assistant_area: chunks[2],
            assistant_inner,
        }
    }
}

impl Default for LayoutBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Application layout - the computed result of layout calculation
///
/// This is the "what it is" - the actual rectangles for each UI element.
/// This is recalculated when the terminal size changes or user preferences change.
#[derive(Debug, Clone, Copy)]
pub struct AppLayout {
    /// Full terminal area
    pub full_area: ratatui::layout::Rect,
    /// Terminal pane area (with borders)
    pub terminal_area: ratatui::layout::Rect,
    /// Terminal pane inner area (without borders)
    pub terminal_inner: ratatui::layout::Rect,
    /// Separator area (the vertical line between panes)
    pub separator_area: ratatui::layout::Rect,
    /// Assistant pane area (with borders)
    pub assistant_area: ratatui::layout::Rect,
    /// Assistant pane inner area (without borders)
    pub assistant_inner: ratatui::layout::Rect,
}
