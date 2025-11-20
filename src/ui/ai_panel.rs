use crate::ui::app::App;
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};

pub fn render_ai_pane(frame: &mut Frame, area: Rect, app: &App) {
    // TODO: get current session history from app.ai_sessions
    let block = Block::default().title("RustyTerm AI").borders(Borders::ALL);
    let para = Paragraph::new("AI chat / suggestions here ...").block(block);
    frame.render_widget(para, area);
}
