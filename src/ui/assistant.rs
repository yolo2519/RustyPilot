use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};
use ratatui::prelude::Rect;
use ratatui::prelude::Buffer;

pub struct TuiAssistant {
    // TODO: implement this
}

impl TuiAssistant {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for TuiAssistant {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for &TuiAssistant {
    fn render(self, area: Rect, buf: &mut Buffer)
    {
        // TODO: this is mocking, implement this
        let fake_assistant_output = vec![
            Line::from("Assistant output will be here.")
        ];
        let paragraph = Paragraph::new(fake_assistant_output);
        paragraph.render(area, buf);
    }
}
