//! User interface module for the TUI application.
//!
//! This module contains all UI-related components including terminal initialization,
//! layout management, panel rendering, application state, and the main event loop.

// use crossterm::style::Stylize;
use ratatui::{
    buffer::Buffer, layout::{Constraint, Direction, Flex, Layout, Rect}, style::{Color, Style, Stylize}, symbols::line, text::Line, widgets::{Block, Borders, Paragraph, Widget}
};
use unicode_width::UnicodeWidthStr;

use crate::app::{ActivePane, App};

pub mod assistant;
pub mod terminal;

impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {

        // Split into three chunks: terminal, separator, assistant
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(60),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(area);

        let active = self.get_active_pane();
        let cmdmode_color = Color::Yellow;
        let active_termcolor = if self.get_command_mode() { cmdmode_color } else { Color::Green };
        let active_aicolor = if self.get_command_mode() { cmdmode_color } else { Color::Cyan };
        let inactive_color = if self.get_command_mode() { cmdmode_color } else { Color::DarkGray };

        // Render terminal pane border
        let block_term = Block::default()
            .title("RustyTerm")
            .borders(Borders::TOP | Borders::BOTTOM | Borders::LEFT)
            .border_style(Style::default().fg(if matches!(active, ActivePane::Terminal) { active_termcolor } else { inactive_color }));
        let term_area = block_term.inner(chunks[0]);
        // Render terminal pane
        self.tui_terminal.render(term_area, buf);


        let side = match active {
            _ if self.get_command_mode() => ActiveSide::None(cmdmode_color),
            ActivePane::Terminal => ActiveSide::Left(active_termcolor),
            ActivePane::Assistant => ActiveSide::Right(active_aicolor),
        };
        // Render separator
        render_separator(chunks[1], buf, side, line::Set::default());

        let block_ai = Block::default()
            .title("Assistant")
            .borders(Borders::TOP | Borders::BOTTOM | Borders::RIGHT)
            .border_style(Style::default().fg(if matches!(active, ActivePane::Assistant) { active_aicolor } else { inactive_color }));
        let ai_area = block_ai.inner(chunks[2]);
        // Render assistant pane
        self.tui_assistant.render(ai_area, buf);

        // Render blocks
        let hint = Line::from(" ^B: Enter Command Mode ");
        let (block_term, block_ai) = match active {
            _ if self.get_command_mode() => (block_term, block_ai),
            ActivePane::Terminal => (block_term.title_bottom(hint.fg(Color::Black).bg(active_termcolor)), block_ai),
            ActivePane::Assistant => (block_term, block_ai.title_bottom(hint.fg(Color::Black).bg(active_aicolor))),
        };

        block_term.render(chunks[0], buf);
        render_separator(chunks[1], buf, side, line::Set::default());
        block_ai.render(chunks[2], buf);
        // Render separator
        if self.get_command_mode() {
            render_command_mode_hint(area, buf, cmdmode_color);
        }
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
fn render_command_mode_hint(screen_area: Rect, buf: &mut Buffer, fg_color: Color) {
    let lines: Vec<(String, String)> = vec![
        (" N".into(), "Toggle active pane".into()),
        (" C".into(), "Exit program".into()),
        (" <Any>".into(),"Quit command mode".into()),
    ];
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
