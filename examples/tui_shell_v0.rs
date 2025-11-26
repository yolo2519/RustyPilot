use std::io::{self, Read, Write, stderr};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::ExecutableCommand;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use ratatui::symbols::line;
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use vt100::Parser;

// UI Configuration
const SIDEBAR_WIDTH: u16 = 60;  // Width of the debug sidebar in columns

fn main() -> Result<()> {
    let mut terminal = ratatui::init();

    // Enable cursor
    std::io::stdout().execute(cursor::Show)?;

    // Get initial terminal size and calculate shell area
    let size = terminal.size()?;
    let (shell_cols, shell_rows) = calculate_shell_size(size.width, size.height);

    let app_result = App::new(shell_cols, shell_rows)?.run(&mut terminal);
    ratatui::restore();
    app_result
}

pub struct App {
    shell: ShellState,
    exit: bool,
    leader_pressed: bool,  // Track if leader key (Ctrl+]) was just pressed
    debug_logs: Vec<String>,  // Debug logs for sidebar
    start_time: std::time::Instant,  // For timing logs
    loop_count: u64,  // Total number of event loop iterations
    last_input_time: Option<std::time::Instant>,  // Last time we received input
}

impl App {
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        Ok(Self {
            shell: ShellState::new(cols, rows)?,
            exit: false,
            leader_pressed: false,
            debug_logs: Vec::new(),
            start_time: std::time::Instant::now(),
            loop_count: 0,
            last_input_time: None,
        })
    }

    fn log_debug(&mut self, msg: String) {
        let elapsed = self.start_time.elapsed();
        let log_entry = format!("[{:7.3}s] {}", elapsed.as_secs_f64(), msg);
        self.debug_logs.push(log_entry);
        // Keep only last 100 logs to avoid memory issues
        if self.debug_logs.len() > 100 {
            self.debug_logs.remove(0);
        }
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut last_draw = std::time::Instant::now();
        let frame_time = Duration::from_millis(16); // Target ~60 FPS

        self.log_debug("Event loop started".to_string());

        while !self.exit {
            self.loop_count += 1;
            let loop_start = std::time::Instant::now();

            // Read any pending output from the shell (non-blocking via mpsc)
            let (has_output, bytes_read, query_logs) = self.shell.read_output_debug();
            if has_output {
                self.log_debug(format!("Shell output: {} bytes", bytes_read));
            }
            // Log any terminal queries detected
            for log in query_logs {
                self.log_debug(log);
            }

            // Check if shell has exited
            if self.shell.is_shell_exited() {
                self.log_debug("Shell exited".to_string());
                self.exit = true;
                break;
            }

            // Check for keyboard input (very short timeout for responsiveness)
            let poll_start = std::time::Instant::now();
            let has_input = event::poll(Duration::from_millis(1))?;
            let poll_duration = poll_start.elapsed();

            if has_input {
                self.last_input_time = Some(std::time::Instant::now());
                self.log_debug(format!("Input detected (poll: {:.1}ms)", poll_duration.as_secs_f64() * 1000.0));
                self.handle_events()?;
                // After handling input, immediately check for shell response
                let (has_resp, resp_bytes, resp_query_logs) = self.shell.read_output_debug();
                if has_resp {
                    self.log_debug(format!("Immediate response: {} bytes", resp_bytes));
                }
                for log in resp_query_logs {
                    self.log_debug(log);
                }
            }

            // Draw if needed: when there's output, input, or enough time has passed
            let should_draw = has_output || has_input || (loop_start - last_draw >= frame_time);
            if should_draw {
                terminal.draw(|frame| self.draw(frame))?;
                last_draw = loop_start;
            }

            // Small sleep to prevent busy-waiting and reduce CPU usage
            // Only sleep if we didn't have any activity
            if !has_output && !has_input {
                thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Render the widget
        frame.render_widget(&*self, area);

        // Set cursor position and make it visible
        if let Some((cursor_x, cursor_y)) = self.get_cursor_position(area) {
            frame.set_cursor_position((cursor_x, cursor_y));
            // Ensure cursor is shown
            let _ = std::io::stdout().execute(cursor::Show);
            // Optionally set cursor style to blinking block (default for most shells)
            let _ = std::io::stdout().execute(cursor::SetCursorStyle::BlinkingBlock);
        }
    }

    fn get_cursor_position(&self, area: Rect) -> Option<(u16, u16)> {
        // Get cursor position from vt100 parser
        let screen = self.shell.parser.screen();
        let cursor = screen.cursor_position();

        // Calculate the shell area (same logic as in render)
        // Account for outer block borders (top border = 1 row)
        let inner_area = Rect {
            x: area.x,
            y: area.y + 1,  // Skip top border
            width: area.width,
            height: area.height.saturating_sub(2),  // Skip top and bottom borders
        };

        // Shell is on the left side (subtract sidebar width)
        let shell_width = inner_area.width.saturating_sub(SIDEBAR_WIDTH);

        // Convert parser coordinates to screen coordinates
        let cursor_x = inner_area.x + cursor.1 as u16;
        let cursor_y = inner_area.y + cursor.0 as u16;

        // Make sure cursor is within the shell area
        if cursor_x < inner_area.x + shell_width && cursor_y < inner_area.y + inner_area.height {
            Some((cursor_x, cursor_y))
        } else {
            None
        }
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self) -> Result<()> {
        if !event::poll(Duration::ZERO)? {
            return Ok(());
        }
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                // Log the key event details
                let event_desc = format_key_event(&key_event);
                self.log_debug(format!("Key: {}", event_desc));
                self.handle_key_event(key_event)?;
            }
            Event::Resize(cols, rows) => {
                self.log_debug(format!("Resize: {}x{}", cols, rows));
                // Calculate the actual shell area size (left side only)
                let (shell_cols, shell_rows) = calculate_shell_size(cols, rows);
                // Update PTY size when terminal is resized
                self.shell.resize(shell_cols, shell_rows)?;
            }
            Event::Mouse(mouse_event) => {
                self.log_debug(format!("Mouse: {:?}", mouse_event));
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        // Check if this is the leader key (Ctrl+] or Ctrl+5)
        // Note: Due to crossterm's parsing, both Ctrl+] and Ctrl+5 produce the same byte (0x1D)
        // and crossterm reports it as Ctrl+5. We accept both to handle this correctly.
        if key_event.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key_event.code, KeyCode::Char(']') | KeyCode::Char('5'))
            && !self.leader_pressed {
            self.log_debug("Action: Enter leader mode".to_string());
            // Leader key pressed, wait for next command
            self.leader_pressed = true;
            return Ok(());
        }

        // If leader was pressed, handle leader commands
        if self.leader_pressed {
            self.log_debug("Action: Leader command".to_string());
            self.leader_pressed = false;  // Reset leader state
            return self.handle_leader_command(key_event);
        }

        // Normal key, forward to shell
        self.shell.send_key(key_event)?;
        Ok(())
    }

    fn handle_leader_command(&mut self, key_event: KeyEvent) -> Result<()> {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                // Ctrl+] Q: Quit application
                self.exit();
            }
            KeyCode::Char('?') => {
                // Ctrl+] ?: Show help (TODO: implement help overlay)
                // For now, just do nothing
            }
            KeyCode::Char(']') | KeyCode::Char('5') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+] Ctrl+] (or Ctrl+5 Ctrl+5): Send literal Ctrl+] (0x1D) to shell
                self.shell.send_raw_bytes(&[0x1D])?;
            }
            KeyCode::Esc => {
                // ESC: Cancel leader mode (already reset above, just explicit)
            }
            _ => {
                // Unknown command - just cancel (do nothing)
                // Leader state was already reset before calling this function
            }
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Render the outer block with top and bottom borders
        let (top_title, bottom_title) = if self.leader_pressed {
            // Leader key was pressed, show available commands
            let top = Line::from(" RustyTerm [COMMAND MODE] ".bold())
                .style(Style::new().bg(Color::Yellow).fg(Color::Black));
            let bottom = Line::from(" Quit(q) | Help(?) | Send ^](^])| Cancel(Any) ")
                .style(Style::new().bg(Color::Yellow).fg(Color::Black));
            (top, bottom)
        } else {
            // Normal mode, show how to activate leader key
            let top = Line::from(" RustyTerm ".bold())
                .style(Style::new().bg(Color::DarkGray));
            let bottom = Line::from(" Commands(^])")
                .style(Style::new().bg(Color::DarkGray));
            (top, bottom)
        };

        let outer_block = Block::new()
            .borders(Borders::TOP | Borders::BOTTOM)
            .title(top_title)
            .title_bottom(bottom_title)
            .border_set(border::FULL);

        let inner_area = outer_block.inner(area);
        outer_block.render(area, buf);

        // Split the inner area into left (shell) and right (sidebar)
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),      // Left: shell (takes remaining space)
                Constraint::Length(SIDEBAR_WIDTH),  // Right: sidebar
            ])
            .split(inner_area);

        // Render left side: shell terminal (no borders)
        self.render_shell(chunks[0], buf);

        // Render right side: sidebar (with special left border)
        self.render_sidebar(chunks[1], buf);
    }
}

impl App {
    fn render_shell(&self, area: Rect, buf: &mut Buffer) {
        // Get shell output as styled lines
        let shell_output = self.shell.get_lines();

        // Render shell without additional borders (outer block already has top/bottom)
        Paragraph::new(shell_output)
            .render(area, buf);
    }

    fn render_sidebar(&self, area: Rect, buf: &mut Buffer) {
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

        let block = Block::new()
            .borders(Borders::LEFT)  // Only show the left border
            .title(" Debug Log ")
            .border_set(custom_border)
            .border_style(Style::default().fg(Color::Cyan));

        // Build stats header
        let mut lines: Vec<Line> = Vec::new();

        // Show statistics
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let loops_per_sec = self.loop_count as f64 / elapsed;

        lines.push(Line::from(format!("Runtime: {:.1}s", elapsed)));
        lines.push(Line::from(format!("Loop: {} ({:.0}/s)", self.loop_count, loops_per_sec)));
        lines.push(Line::from(format!("Total RX: {} bytes", self.shell.total_bytes_received)));

        if let Some(last_time) = self.shell.last_output_time {
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

        // Show recent debug logs (most recent at bottom)
        let available_lines = area.height.saturating_sub(8) as usize;  // Reserve space for stats
        let log_lines = self.debug_logs
            .iter()
            .rev()  // Reverse so newest is at bottom
            .take(available_lines)
            .rev()  // Reverse back so chronological order
            .map(|log| Line::from(log.as_str()));

        lines.extend(log_lines);

        Paragraph::new(lines)
            .block(block)
            .render(area, buf);
    }
}

// ============================================================================
// Shell State Management
// ============================================================================

pub struct ShellState {
    pub parser: Parser,  // Make public so we can access cursor position
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    pty_master: Box<dyn MasterPty + Send>,
    shell_exited: bool,  // Track if the shell process has exited
    last_output_time: Option<std::time::Instant>,  // Debug: track when last output arrived
    total_bytes_received: usize,  // Debug: total bytes received from shell
}

impl ShellState {
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        let pty_system = native_pty_system();

        // Create a new PTY
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Spawn zsh in the PTY
        let mut cmd = CommandBuilder::new("zsh");
        cmd.env("TERM", "xterm-256color");
        let _child = pair.slave.spawn_command(cmd)?;

        // Drop the slave side in the parent process
        drop(pair.slave);

        let writer = pair.master.take_writer()?;
        let reader = pair.master.try_clone_reader()?;
        let pty_master = pair.master;

        // Create mpsc channel for output
        let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>();

        // Spawn reader thread
        thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF - PTY closed, send empty vec as EOF marker
                        let _ = output_tx.send(vec![]);
                        break;
                    }
                    Ok(n) => {
                        // Send the data to the main thread
                        let data = buf[..n].to_vec();
                        if output_tx.send(data).is_err() {
                            // Main thread dropped the receiver, exit
                            break;
                        }
                    }
                    Err(e) => {
                        // Handle errors - for now just log and continue
                        eprintln!("PTY read error: {}", e);
                        if e.kind() != io::ErrorKind::Interrupted {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            parser: Parser::new(rows, cols, 1000), // 1000 lines of scrollback
            writer,
            output_rx,
            pty_master,
            shell_exited: false,
            last_output_time: None,
            total_bytes_received: 0,
        })
    }

    pub fn read_output(&mut self) -> bool {
        let (has_output, _, _) = self.read_output_debug();
        has_output
    }

    pub fn read_output_debug(&mut self) -> (bool, usize, Vec<String>) {
        // Non-blocking: read all available data from the channel
        let mut has_output = false;
        let mut total_bytes = 0;
        let mut query_logs = Vec::new();

        while let Ok(data) = self.output_rx.try_recv() {
            if data.is_empty() {
                // Empty vec is EOF marker - shell has exited
                self.shell_exited = true;
                has_output = true;
                break;
            }
            total_bytes += data.len();
            self.total_bytes_received += data.len();
            self.last_output_time = Some(std::time::Instant::now());

            // Check for and respond to terminal queries, get filtered data
            let (filtered_data, log) = self.handle_terminal_queries(&data);
            if let Some(log_msg) = log {
                query_logs.push(log_msg);
            }

            // Feed only filtered output to the VT100 parser (queries removed)
            if !filtered_data.is_empty() {
                self.parser.process(&filtered_data);
                has_output = true;
            } else if !query_logs.is_empty() {
                // We had queries but no display data
                has_output = true;
            }
        }
        (has_output, total_bytes, query_logs)
    }

    fn handle_terminal_queries(&mut self, data: &[u8]) -> (Vec<u8>, Option<String>) {
        // Look for common terminal query sequences and filter them out
        // CPR (Cursor Position Report): ESC[6n
        // DA (Device Attributes): ESC[c or ESC[>c

        let mut data_str = String::from_utf8_lossy(data).to_string();
        let mut log_msg = None;

        // Check for queries and respond, then filter them out

        // ESC[6n - Cursor Position Report query
        if data_str.contains("\x1b[6n") {
            let screen = self.parser.screen();
            let cursor = screen.cursor_position();
            let response = format!("\x1b[{};{}R", cursor.0 + 1, cursor.1 + 1);
            let _ = self.writer.write_all(response.as_bytes());
            let _ = self.writer.flush();
            log_msg = Some(format!("Query: CPR -> resp[{},{}]", cursor.0 + 1, cursor.1 + 1));
            data_str = data_str.replace("\x1b[6n", "");
        }

        // ESC[c - Primary Device Attributes query (be careful with order!)
        if data_str.contains("\x1b[0c") {
            let response = "\x1b[?1;2c";
            let _ = self.writer.write_all(response.as_bytes());
            let _ = self.writer.flush();
            log_msg = Some("Query: DA (primary)".to_string());
            data_str = data_str.replace("\x1b[0c", "");
        } else if data_str.contains("\x1b[c") && !data_str.contains("\x1b[>") {
            let response = "\x1b[?1;2c";
            let _ = self.writer.write_all(response.as_bytes());
            let _ = self.writer.flush();
            log_msg = Some("Query: DA (primary)".to_string());
            data_str = data_str.replace("\x1b[c", "");
        }

        // ESC[>c - Secondary Device Attributes query
        if data_str.contains("\x1b[>0c") {
            let response = "\x1b[>0;276;0c";
            let _ = self.writer.write_all(response.as_bytes());
            let _ = self.writer.flush();
            log_msg = Some("Query: DA (secondary)".to_string());
            data_str = data_str.replace("\x1b[>0c", "");
        } else if data_str.contains("\x1b[>c") {
            let response = "\x1b[>0;276;0c";
            let _ = self.writer.write_all(response.as_bytes());
            let _ = self.writer.flush();
            log_msg = Some("Query: DA (secondary)".to_string());
            data_str = data_str.replace("\x1b[>c", "");
        }

        // Filter out our own responses if they got echoed back
        data_str = data_str.replace("\x1b[?1;2c", "");
        data_str = data_str.replace("\x1b[>0;276;0c", "");

        (data_str.into_bytes(), log_msg)
    }

    pub fn is_shell_exited(&self) -> bool {
        self.shell_exited
    }

    pub fn send_key(&mut self, key_event: KeyEvent) -> Result<()> {
        let bytes = key_to_bytes(key_event);
        self.writer.write_all(&bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn send_raw_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        // Resize the PTY
        self.pty_master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Create a new parser with the new size
        // Note: This will lose scrollback history
        self.parser = Parser::new(rows, cols, 1000);

        Ok(())
    }

    pub fn get_lines(&'_ self) -> Vec<Line<'_>> {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();

        let mut lines = Vec::new();
        for row in 0..rows {
            let mut spans = Vec::new();
            let mut current_text = String::new();
            let mut current_style = Style::default();

            for col in 0..cols {
                let cell = screen.cell(row, col).unwrap();
                let contents = cell.contents();

                // Determine what to display:
                // - If cell has content: display it
                // - If cell is empty: check if it's a wide character placeholder
                //   * If previous cell had a wide character, this is a placeholder (skip it)
                //   * Otherwise, it's a real empty cell (render space for background color)

                let display_contents = if contents.is_empty() {
                    // Check if previous cell has wide character
                    if col > 0 {
                        if let Some(prev_cell) = screen.cell(row, col - 1) {
                            let prev_contents = prev_cell.contents();
                            if !prev_contents.is_empty() {
                                // Check if previous character is wide (CJK, emoji, etc.)
                                let is_wide = prev_contents.chars().next()
                                    .map(|c| {
                                        // Use unicode-width crate to determine width
                                        use unicode_width::UnicodeWidthChar;
                                        c.width().unwrap_or(1) > 1
                                    })
                                    .unwrap_or(false);

                                if is_wide {
                                    // This is a wide character placeholder, skip it
                                    continue;
                                }
                            }
                        }
                    }
                    // Real empty cell, render space to preserve background color
                    " "
                } else {
                    &contents
                };

                // Build style for this cell
                let mut style = Style::default();

                // Convert colors
                style = style.fg(convert_color(cell.fgcolor()));
                style = style.bg(convert_color(cell.bgcolor()));

                // Add modifiers
                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.italic() {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.underline() {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.inverse() {
                    style = style.add_modifier(Modifier::REVERSED);
                }

                // Group consecutive cells with the same style
                if style == current_style {
                    current_text.push_str(display_contents);
                } else {
                    if !current_text.is_empty() {
                        spans.push(Span::styled(current_text.clone(), current_style));
                        current_text.clear();
                    }
                    current_style = style;
                    current_text.push_str(display_contents);
                }
            }

            // Add the last span
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text, current_style));
            }

            lines.push(Line::from(spans));
        }

        lines
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn format_key_event(key_event: &KeyEvent) -> String {
    let mut parts = Vec::new();

    // Add modifiers
    if key_event.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if key_event.modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt");
    }
    if key_event.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift");
    }

    // Add key code
    let key_str = match key_event.code {
        KeyCode::Char(c) => {
            if key_event.modifiers.contains(KeyModifiers::CONTROL) && c.is_ascii_lowercase() {
                // Calculate control byte
                let byte = (c.to_ascii_lowercase() as u8) - b'a' + 1;
                format!("{}(0x{:02x})", c, byte)
            } else {
                format!("'{}'", c)
            }
        }
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        KeyCode::Delete => "Del".to_string(),
        KeyCode::Insert => "Ins".to_string(),
        KeyCode::F(n) => format!("F{}", n),
        _ => format!("{:?}", key_event.code),
    };

    if parts.is_empty() {
        key_str
    } else {
        format!("{}+{}", parts.join("+"), key_str)
    }
}

fn convert_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(idx) => {
            match idx {
                0 => Color::Black,
                1 => Color::Red,
                2 => Color::Green,
                3 => Color::Yellow,
                4 => Color::Blue,
                5 => Color::Magenta,
                6 => Color::Cyan,
                7 => Color::Gray,
                8 => Color::DarkGray,
                9 => Color::LightRed,
                10 => Color::LightGreen,
                11 => Color::LightYellow,
                12 => Color::LightBlue,
                13 => Color::LightMagenta,
                14 => Color::LightCyan,
                15 => Color::White,
                _ => Color::Indexed(idx),
            }
        }
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn key_to_bytes(key_event: KeyEvent) -> Vec<u8> {
    let KeyEvent { code, modifiers, .. } = key_event;

    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);

    // eprintln!("DEBUG key_to_bytes: code={:?}, ctrl={}, alt={}", code, ctrl, alt);
    let _ = stderr().flush();

    match code {
        KeyCode::Char(c) => {
            // eprintln!("DEBUG: Char '{}' (0x{:02x})", c, c as u8);
            let _ = stderr().flush();
            if ctrl {
                // Handle Ctrl+letter: Ctrl+A = 0x01, Ctrl+B = 0x02, etc.
                if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
                    let byte = (c.to_ascii_lowercase() as u8) - b'a' + 1;
                    vec![byte]
                }
                // Handle special Ctrl combinations
                else if c == '@' {
                    vec![0x00]  // Ctrl+@ = NUL
                }
                else if c == '[' {
                    vec![0x1b]  // Ctrl+[ = ESC
                }
                else if c == '\\' {
                    vec![0x1c]  // Ctrl+\ = FS
                }
                else if c == ']' {
                    vec![0x1d]  // Ctrl+] = GS
                }
                else if c == '^' {
                    vec![0x1e]  // Ctrl+^ = RS
                }
                else if c == '_' {
                    vec![0x1f]  // Ctrl+_ = US
                }
                else if c == '?' {
                    vec![0x7f]  // Ctrl+? = DEL
                }
                else {
                    // For other characters, just send the character itself
                    c.to_string().into_bytes()
                }
            } else if alt {
                vec![0x1b, c as u8]  // ESC + char
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Left => vec![0x1b, b'[', b'D'],
        KeyCode::Home => vec![0x1b, b'[', b'H'],
        KeyCode::End => vec![0x1b, b'[', b'F'],
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'],
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],
        KeyCode::F(n) if n >= 1 && n <= 12 => {
            match n {
                1 => vec![0x1b, b'O', b'P'],
                2 => vec![0x1b, b'O', b'Q'],
                3 => vec![0x1b, b'O', b'R'],
                4 => vec![0x1b, b'O', b'S'],
                5..=12 => format!("\x1b[{}~", n + 10).into_bytes(),
                _ => vec![],
            }
        }
        _ => vec![],
    }
}

/// Calculate the actual shell area size based on terminal dimensions
/// This accounts for:
/// - Top and bottom borders (2 rows)
/// - Right sidebar width (configured by SIDEBAR_WIDTH constant)
fn calculate_shell_size(terminal_cols: u16, terminal_rows: u16) -> (u16, u16) {
    const BORDER_HEIGHT: u16 = 2;  // Top + Bottom borders

    // Calculate available columns (subtract sidebar width)
    let cols = if terminal_cols > SIDEBAR_WIDTH {
        terminal_cols - SIDEBAR_WIDTH
    } else {
        1  // Minimum 1 column
    };

    // Calculate available rows (subtract borders)
    let rows = if terminal_rows > BORDER_HEIGHT {
        terminal_rows - BORDER_HEIGHT
    } else {
        1  // Minimum 1 row
    };

    (cols, rows)
}
