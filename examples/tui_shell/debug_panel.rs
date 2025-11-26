//! Debug panel widget for displaying diagnostic information and logs.

use std::time::Instant;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    symbols::border,
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};
use ratatui::symbols::line;

/// Shell statistics for display in debug panel.
#[derive(Debug, Clone)]
pub struct ShellStats {
    pub total_bytes_received: usize,
    pub last_output_time: Option<Instant>,
}

/// Debug panel widget that displays logs and statistics.
pub struct DebugPanel {
    logs: Vec<String>,
    start_time: Instant,
    scroll_offset: usize,  // Number of lines scrolled back (0 = at bottom/latest)
    last_input_time: Option<std::time::Instant>,  // Last time we received input
    last_fps_update: Instant,  // Last time we updated FPS
    frames_this_second: u32,  // Frame count in current sampling period
    last_fps: f64,  // Cached FPS value (updated every second)
    last_loop_update: Instant,  // Last time we updated loop count
    loops_this_second: u64,  // Loop count in current sampling period
    last_loops_per_sec: f64,  // Cached loops/sec value (updated every second)
}

impl DebugPanel {
    /// Create a new debug panel.
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            logs: Vec::new(),
            start_time: now,
            scroll_offset: 0,
            last_input_time: None,
            last_fps_update: now,
            frames_this_second: 0,
            last_fps: 0.0,
            last_loop_update: now,
            loops_this_second: 0,
            last_loops_per_sec: 0.0,
        }
    }

    /// Increments the loop counter and updates loops per second statistics.
    ///
    /// # Arguments
    /// * `count` - Number of loops to add to the counter
    pub fn increase_loop_count(&mut self, count: u64) {
        let now = Instant::now();
        self.loops_this_second += count;

        // Update loops/sec every second
        let elapsed = now.duration_since(self.last_loop_update).as_secs_f64();
        if elapsed >= 1.0 {
            self.last_loops_per_sec = self.loops_this_second as f64 / elapsed;
            self.loops_this_second = 0;
            self.last_loop_update = now;
        }
    }

    /// Updates the timestamp of the last received input.
    ///
    /// # Arguments
    /// * `time` - The timestamp of the input event
    pub fn set_last_input_time(&mut self, time: Instant) {
        self.last_input_time.replace(time);
    }

    /// Record a frame render event for FPS calculation.
    /// Samples FPS every second based on frame count.
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        self.frames_this_second += 1;

        // Update FPS every second
        let elapsed = now.duration_since(self.last_fps_update).as_secs_f64();
        if elapsed >= 1.0 {
            self.last_fps = self.frames_this_second as f64 / elapsed;
            self.frames_this_second = 0;
            self.last_fps_update = now;
        }
    }

    /// Scroll up by n lines (into history)
    pub fn scroll_up(&mut self, n: usize) {
        // Limit scroll to total log count
        self.scroll_offset = (self.scroll_offset + n).min(self.logs.len());
    }

    /// Scroll down by n lines (toward latest)
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Reset scroll to bottom (latest output)
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Check if we're scrolled back in history
    pub fn is_scrolled(&self) -> bool {
        self.scroll_offset > 0
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Add a log message to the panel.
    /// Automatically keeps only the last 100 messages to prevent memory issues.
    pub fn add_log(&mut self, msg: impl AsRef<str>) {
        let elapsed = self.start_time.elapsed();
        let log_entry = format!("[{:7.3}s] {}", elapsed.as_secs_f64(), msg.as_ref());
        self.logs.push(log_entry);

        // Keep only last 100 logs to avoid memory issues
        if self.logs.len() > 100 {
            self.logs.remove(0);
        }
    }

    /// Render the debug panel.
    pub fn render(
        &self,
        area: Rect,
        buf: &mut Buffer,
        shell_stats: &ShellStats,
        is_active: bool,
        is_browsing: bool,
    ) {
        // Custom border set with a special left border
        let custom_border = border::Set {
            top_left: line::VERTICAL,           // Connect to top border
            bottom_left: line::VERTICAL,        // Connect to bottom border
            vertical_left: line::VERTICAL,      // Special thick/double line for left border
            top_right: " ",                     // No right border
            bottom_right: " ",
            vertical_right: " ",
            horizontal_top: " ",                // No top/bottom (already handled by outer block)
            horizontal_bottom: " ",
        };

        // Highlight border if active in browsing mode
        let border_color = if is_active && is_browsing {
            Color::Yellow
        } else {
            Color::Cyan
        };

        let title = if is_active {
            " Debug Log [ACTIVE] "
        } else {
            " Debug Log "
        };

        let block = Block::new()
            .borders(Borders::LEFT)  // Only show the left border
            .title(title)
            .border_set(custom_border)
            .border_style(Style::default().fg(border_color));

        // Build stats header
        let mut lines: Vec<Line> = Vec::new();

        // Show statistics
        let elapsed = self.start_time.elapsed().as_secs_f64();

        // Display FPS with color coding
        let fps_line = if self.last_fps > 0.0 {
            let fps_style = if self.last_fps >= 60.0 {
                Style::default().fg(Color::Green)
            } else if self.last_fps >= 30.0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Red)
            };
            Line::from(format!("FPS: {:.1}", self.last_fps)).style(fps_style)
        } else {
            Line::from("FPS: --")
        };

        lines.push(fps_line);
        lines.push(Line::from(format!("Runtime: {:.1}s", elapsed)));

        // Display loops/sec from last second
        let loops_line = if self.last_loops_per_sec > 0.0 {
            Line::from(format!("Loop: {:.0}/s", self.last_loops_per_sec))
        } else {
            Line::from("Loop: --")
        };
        lines.push(loops_line);

        lines.push(Line::from(format!("Total RX: {} bytes", shell_stats.total_bytes_received)));

        if let Some(last_time) = shell_stats.last_output_time {
            let ago = last_time.elapsed().as_millis();
            lines.push(Line::from(format!("Last output: {}ms ago", ago))
                .style(if ago < 100 { Style::default().fg(Color::Green) }
                       else { Style::default() }));
        } else {
            lines.push(Line::from("Last output: never"));
        }

        if let Some(last_time) = self.last_input_time {
            let ago = last_time.elapsed().as_millis();
            lines.push(Line::from(format!("Last input: {}ms ago", ago))
                .style(if ago < 100 { Style::default().fg(Color::Yellow) }
                       else { Style::default() }));
        } else {
            lines.push(Line::from("Last input: never"));
        }

        lines.push(Line::from("---"));

        // Show debug logs with scroll support
        let available_lines = area.height.saturating_sub(8) as usize;  // Reserve space for stats

        // Calculate which logs to show based on scroll offset
        let total_logs = self.logs.len();
        if self.scroll_offset > 0 {
            // Scrolled back: show older logs
            let end_idx = total_logs.saturating_sub(self.scroll_offset);
            let start_idx = end_idx.saturating_sub(available_lines);
            let log_lines = self.logs[start_idx..end_idx]
                .iter()
                .map(|log| Line::from(log.as_str()));
            lines.extend(log_lines);
        } else {
            // At bottom: show most recent logs
            let log_lines = self.logs
                .iter()
                .rev()  // Reverse so newest is at bottom
                .take(available_lines)
                .rev()  // Reverse back so chronological order
                .map(|log| Line::from(log.as_str()));
            lines.extend(log_lines);
        }

        // Add subtle background highlight if active in browsing mode
        let paragraph = if is_active && is_browsing {
            Paragraph::new(lines)
                .block(block)
                .style(Style::default().bg(Color::Rgb(30, 20, 20)))
        } else {
            Paragraph::new(lines).block(block)
        };

        Widget::render(paragraph, area, buf);
    }
}
