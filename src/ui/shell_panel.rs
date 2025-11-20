use crate::ui::app::App;
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_shell_pane(frame: &mut Frame, area: Rect, app: &App) {
    // TODO: 根据 app.shell_manager 中的 buffer 渲染
    let block = Block::default().title("Shell").borders(Borders::ALL);
    let para = Paragraph::new("Shell output here ...").block(block);
    frame.render_widget(para, area);
}
