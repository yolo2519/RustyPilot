//! User interface module for the TUI application.
//!
//! This module contains all UI-related components including terminal initialization,
//! layout management, panel rendering, application state, and the main event loop.

// use crossterm::style::Stylize;
use ratatui::{
    buffer::Buffer,
    layout::{
        Constraint, Flex, Layout, Rect
    },
    style::{
        Color, Style, Stylize
    },
    symbols::line,
    text::Line,
    widgets::{
        Block, Borders, Paragraph, Widget
    },
};
use unicode_width::UnicodeWidthStr;

use crate::app::{ActivePane, App};

pub mod assistant;
pub mod layout;
pub mod terminal;
pub mod visual;

impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        // Use the pre-calculated layout
        let layout = self.layout();

        let active = self.get_active_pane();
        let cmdmode_color = Color::Yellow;

        // Get pane status from components (they own their visual mode state)
        let term_status = self.tui_terminal.get_pane_status();
        let ai_status = self.tui_assistant.get_pane_status();

        // Determine colors based on mode
        let default_term_color = Color::Green;
        let default_ai_color = Color::Cyan;
        let inactive_color = Color::DarkGray;

        let (active_termcolor, active_aicolor) = if self.get_command_mode() {
            (cmdmode_color, cmdmode_color)
        } else {
            (
                term_status.border_color.unwrap_or(default_term_color),
                ai_status.border_color.unwrap_or(default_ai_color),
            )
        };

        // Build terminal title with status from component
        let term_title = build_pane_title("RustyTerm", &term_status.title_status);
        let block_term = Block::default()
            .title(term_title)
            .borders(Borders::TOP | Borders::BOTTOM | Borders::LEFT)
            .border_style(Style::default().fg(if matches!(active, ActivePane::Terminal) { active_termcolor } else { inactive_color }));
        let term_area = layout.terminal_inner;
        // Render terminal pane
        self.tui_terminal.render(term_area, buf);

        // Determine separator style
        let side = match active {
            _ if self.get_command_mode() => ActiveSide::None(cmdmode_color),
            ActivePane::Terminal => {
                if term_status.border_color.is_some() {
                    ActiveSide::None(active_termcolor)
                } else {
                    ActiveSide::Left(active_termcolor)
                }
            }
            ActivePane::Assistant => {
                if ai_status.border_color.is_some() {
                    ActiveSide::None(active_aicolor)
                } else {
                    ActiveSide::Right(active_aicolor)
                }
            }
        };
        // Render separator
        render_separator(layout.separator_area, buf, side, line::Set::default());

        // Build assistant title with status from component
        let ai_title = build_pane_title("Assistant", &ai_status.title_status);
        let block_ai = Block::default()
            .title(ai_title)
            .borders(Borders::TOP | Borders::BOTTOM | Borders::RIGHT)
            .border_style(Style::default().fg(if matches!(active, ActivePane::Assistant) { active_aicolor } else { inactive_color }));
        let ai_area = layout.assistant_inner;
        // Render assistant pane
        self.tui_assistant.render(ai_area, buf);

        // Determine bottom hint from active pane's status
        let (hint, hint_color) = match active {
            ActivePane::Terminal => {
                let hint = term_status.hint_text.unwrap_or(" Ctrl + B: Enter Command Mode ");
                (hint, active_termcolor)
            }
            ActivePane::Assistant => {
                let hint = ai_status.hint_text.unwrap_or(" Ctrl + B: Enter Command Mode ");
                (hint, active_aicolor)
            }
        };

        let (block_term, block_ai) = match active {
            _ if self.get_command_mode() => (block_term, block_ai),
            ActivePane::Terminal => (block_term.title_bottom(hint.fg(Color::Black).bg(hint_color)), block_ai),
            ActivePane::Assistant => (block_term, block_ai.title_bottom(hint.fg(Color::Black).bg(hint_color))),
        };

        block_term.render(layout.terminal_area, buf);
        render_separator(layout.separator_area, buf, side, line::Set::default());
        block_ai.render(layout.assistant_area, buf);

        // Render command mode popup if active
        if self.get_command_mode() {
            let extra_hints: Vec<(String, String)> = match active {
                ActivePane::Terminal => vec![
                    (" ^B".into(), "Send ^B to shell".into()),
                    (" V".into(), "Enter Visual mode".into()),
                ],
                ActivePane::Assistant => vec![
                    (" T".into(), "New session".into()),
                    (" W".into(), "Close session".into()),
                    (" ]".into(), "Next session".into()),
                    (" [".into(), "Previous session".into()),
                    (" V".into(), "Enter Visual mode".into()),
                ],
            };
            render_command_mode_hint(area, buf, cmdmode_color, extra_hints);
        }
    }
}

/// Build a pane title with optional status suffix.
fn build_pane_title(base_name: &str, status: &Option<String>) -> String {
    match status {
        Some(s) => format!("{} [{}]", base_name, s),
        None => base_name.to_string(),
    }
}

#[derive(Clone, Copy, Debug)]
enum ActiveSide {
    None(Color),
    Left(Color),
    Right(Color),
}

/// Renders a vertical separator line between terminal and assistant panes.
/// The separator's appearance changes based on which pane is active.
fn render_separator(area: Rect, buf: &mut Buffer, side: ActiveSide, line_set: line::Set) {
    let height = area.height as usize;
    let (top, vertical, bottom, color) = match side {
        ActiveSide::None(color) => (line_set.horizontal_down, line_set.vertical, line_set.horizontal_up, color),
        ActiveSide::Left(color) => (line_set.top_right, line_set.vertical, line_set.bottom_right, color),
        ActiveSide::Right(color) => (line_set.top_left, line_set.vertical, line_set.bottom_left, color),
    };

    let mut lines = Vec::with_capacity(height);
    lines.resize_with(height, || Line::from(vertical));
    lines[0] = Line::from(top);
    lines[height - 1] = Line::from(bottom);

    let separator_style = Style::default().fg(color);

    let separator = Paragraph::new(lines).style(separator_style);
    separator.render(area, buf);
}

/// helper function to create a centered rect using up certain percentage of the available rect `r`
fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

/// Render a pop-up with command mode hints
fn render_command_mode_hint(screen_area: Rect, buf: &mut Buffer, fg_color: Color, extra_hints: impl IntoIterator<Item = (String, String)>) {
    let mut lines: Vec<(String, String)> = vec![
        (" N".into(), "Toggle active pane".into()),
        (" C".into(), "Exit program".into()),
        (" L".into(), "Force redraw (clear screen)".into()),
        (" <Any>".into(),"Quit command mode".into())
    ];

    lines.extend(extra_hints);

    let max_key_width = lines.iter().map(|(a, _)| a.width()).max().unwrap_or(0);
    let max_value_width = lines.iter().map(|(_, b)| b.width()).max().unwrap_or(0);
    let lines: Vec<Line<'_>> = lines.into_iter().map(|(mut k, v)| {
        k.extend(std::iter::repeat_n(' ', max_key_width - k.width()));
        k.push_str(": ");
        k.push_str(&v);
        k.extend(std::iter::repeat_n(' ', max_value_width - v.width()));
        k.push(' ');
        Line::from(k)
    }).collect();
    let required_height = lines.len() + 2;
    let required_width = lines.iter().map(|l| l.width()).max().unwrap_or(0) + 2;
    let paragraph = Paragraph::new(lines);
    let area = popup_area(screen_area, required_width as u16, required_height as u16);
    let block = Block::new()
        .borders(Borders::all())
        .title(" COMMAND MODE KEYMAP ")
        .title_alignment(ratatui::layout::Alignment::Center)
        .bg(Color::DarkGray)
        .fg(fg_color);
    let inner = block.inner(area);
    block.render(area, buf);
    paragraph.render(inner, buf);
}
