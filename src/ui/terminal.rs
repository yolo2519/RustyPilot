use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::prelude::Rect;
use ratatui::prelude::Buffer;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;

use crate::event::AppEvent;

pub struct TuiTerminal {
    // TODO: implement this
    _pty_output: Receiver<Vec<u8>>,
    _event_sink: Sender<AppEvent>,
}

impl TuiTerminal {
    pub fn new(pty_output: Receiver<Vec<u8>>, event_sink: Sender<AppEvent>) -> Self {
        Self {
            _pty_output: pty_output,
            _event_sink: event_sink,
        }
    }
}


impl Widget for &TuiTerminal {
    fn render(self, area: Rect, buf: &mut Buffer)
    {
        // TODO: this is mocking, implement this
        let fake_terminal_output = vec![
            Line::from("Terminal will be here."),
            Line::from("[DEBUG] Press Ctrl + C to exit."),
            Line::from("[DEBUG] Press Ctrl + B to enter Command Mode"),
            Line::from("[DEBUG] In command mode: Press N to toggle active pane."),
            Line::from("[DEBUG] In command mode: Press C to exit."),
        ];
        let paragraph = Paragraph::new(fake_terminal_output);
        paragraph.render(area, buf);
    }
}
