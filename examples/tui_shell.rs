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
}

impl App {
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        Ok(Self {
            shell: ShellState::new(cols, rows)?,
            exit: false,
            leader_pressed: false,
        })
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.exit {
            // Read any pending output from the shell (non-blocking via mpsc)
            self.shell.read_output();
            
            terminal.draw(|frame| self.draw(frame))?;
            
            // Use non-blocking event check
            if event::poll(Duration::from_millis(50))? {
                self.handle_events()?;
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
        
        // Shell is on the left side (subtract sidebar width of 40)
        let shell_width = inner_area.width.saturating_sub(40);
        
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
                self.handle_key_event(key_event)?;
            }
            Event::Resize(cols, rows) => {
                // Calculate the actual shell area size (left side only)
                let (shell_cols, shell_rows) = calculate_shell_size(cols, rows);
                // Update PTY size when terminal is resized
                self.shell.resize(shell_cols, shell_rows)?;
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        // DEBUG: Log key events
        // eprintln!("DEBUG: Key event: code={:?}, modifiers={:?}", key_event.code, key_event.modifiers);
        // let _ = stderr().flush();
        
        // Check if this is the leader key (Ctrl+] or Ctrl+5)
        // Note: Due to crossterm's parsing, both Ctrl+] and Ctrl+5 produce the same byte (0x1D)
        // and crossterm reports it as Ctrl+5. We accept both to handle this correctly.
        if key_event.modifiers.contains(KeyModifiers::CONTROL) 
            && matches!(key_event.code, KeyCode::Char(']') | KeyCode::Char('5'))
            && !self.leader_pressed {
            // eprintln!("DEBUG: Leader key detected!");
            // let _ = stderr().flush();
            // Leader key pressed, wait for next command
            self.leader_pressed = true;
            return Ok(());
        }
        
        // eprintln!("DEBUG: Not leader key, forwarding to shell");
        let _ = stderr().flush();

        // If leader was pressed, handle leader commands
        if self.leader_pressed {
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
                Constraint::Length(40),  // Right: sidebar (fixed 40 columns)
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
            // .title(" Info ")
            .border_set(custom_border)
            .border_style(Style::default().fg(Color::Cyan));

        // Empty content for now
        Paragraph::new("")
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
                        // EOF - PTY closed
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
        })
    }

    pub fn read_output(&mut self) {
        // Non-blocking: read all available data from the channel
        while let Ok(data) = self.output_rx.try_recv() {
            // Feed output to the VT100 parser
            self.parser.process(&data);
        }
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
                if style == current_style && !contents.is_empty() {
                    current_text.push_str(&contents);
                } else {
                    if !current_text.is_empty() {
                        spans.push(Span::styled(current_text.clone(), current_style));
                        current_text.clear();
                    }
                    current_style = style;
                    current_text.push_str(&contents);
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
/// - Right sidebar width (40 columns)
fn calculate_shell_size(terminal_cols: u16, terminal_rows: u16) -> (u16, u16) {
    const SIDEBAR_WIDTH: u16 = 40;
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
