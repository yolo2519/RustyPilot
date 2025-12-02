use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::prelude::Rect;
use ratatui::prelude::Buffer;
use tokio::sync::mpsc::{Receiver, UnboundedSender};

use crate::event::AppEvent;

pub struct TuiTerminal {
    pty_output: Receiver<Vec<u8>>,
    _event_sink: UnboundedSender<AppEvent>,
    // Buffer to accumulate output text
    output_buffer: String,
}

impl TuiTerminal {
    pub fn new(pty_output: Receiver<Vec<u8>>, event_sink: UnboundedSender<AppEvent>) -> Self {
        let mut output_buffer = String::new();
        output_buffer.push_str("Terminal Output:\n");
        output_buffer.push_str("[DEBUG] Press Ctrl + B to enter Command Mode\n");
        output_buffer.push_str("[DEBUG] In command mode: Press N to toggle active pane.\n");
        output_buffer.push_str("[DEBUG] In command mode: Press C to exit.\n");
        Self {
            pty_output,
            _event_sink: event_sink,
            output_buffer,
        }
    }

    /// Receive output from the pty stream
    /// This should be awaited in the tokio::select! event loop
    pub async fn recv_pty_output(&mut self) {
        // Await for the next chunk of bytes from the pty
        if let Some(bytes) = self.pty_output.recv().await {
            if let Ok(text) = String::from_utf8(bytes) {
                self.output_buffer.push_str(&text);
            }
        }
    }
}


impl Widget for &TuiTerminal {
    fn render(self, area: Rect, buf: &mut Buffer)
    {
        // Convert accumulated output into lines for display
        let lines: Vec<Line> = self.output_buffer
            .lines()
            .map(|line| Line::from(line.to_string()))
            .collect();
        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}
