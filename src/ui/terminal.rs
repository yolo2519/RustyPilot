use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::prelude::Rect;
use ratatui::prelude::Buffer;

pub struct TuiTerminal {
    // TODO: implement this
}

impl TuiTerminal {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for TuiTerminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for &TuiTerminal {
    fn render(self, area: Rect, buf: &mut Buffer)
    {
        // TODO: this is mocking, implement this
        let fake_terminal_output = vec![
            Line::from("Terminal will be here."),
            Line::from("Press Ctrl + C to exit...")
        ];
        let paragraph = Paragraph::new(fake_terminal_output);
        paragraph.render(area, buf);
    }
}
