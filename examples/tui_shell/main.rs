//! TUI shell example demonstrating terminal emulation with debugging panel.

mod shell;
mod terminal;
mod debug_panel;

use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::ExecutableCommand;
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::border,
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};

use shell::ShellState;
use terminal::TerminalDisplay;
use debug_panel::{DebugPanel, ShellStats};

// UI Configuration
const SIDEBAR_WIDTH: u16 = 60;  // Width of the debug sidebar in columns

/// Which panel is currently active/focused
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePanel {
    Terminal,
    DebugPanel,
}

/// Input mode for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,          // Normal mode: keys are sent to shell (only when Terminal is active)
    LeaderCommand,   // Leader key (Ctrl+]) pressed, waiting for command
    Browsing,        // Browsing mode: arrow keys scroll the active panel, other keys exit
}

type Terminal = ratatui::Terminal<ratatui::prelude::CrosstermBackend<std::io::Stdout>>;

/// Application entry point.
/// Initializes the terminal UI and runs the application main loop.
fn main() -> Result<()> {
    let mut terminal = ratatui::init();

    // Enable cursor and mouse capture
    std::io::stdout().execute(cursor::Show)?;
    // TODO: ^ we don't actually want to show cursor all the time
    // for example if we are in Browsing mode, we want to hide it.

    std::io::stdout().execute(event::EnableMouseCapture)?;

    // Get initial terminal size and calculate shell area

    let app_result = App::new(&terminal)?.run(&mut terminal);
    // Cleanup
    std::io::stdout().execute(event::DisableMouseCapture)?;
    ratatui::restore();
    app_result
}

/// Main application state managing the shell, terminal display, and debug panel.
pub struct App {
    // Backend
    shell: ShellState,

    // Frontend widgets
    term_display: TerminalDisplay,
    debug_panel: DebugPanel,

    // App state
    exit: bool,
    active_panel: ActivePanel,  // Which panel has focus
    input_mode: InputMode,  // Current input mode
}

impl App {
    /// Creates a new application instance.
    ///
    /// # Arguments
    /// * `terminal` - The terminal instance used to get initial size
    ///
    /// # Returns
    /// A new `App` instance with initialized shell, terminal display, and debug panel
    pub fn new(terminal: &Terminal) -> Result<Self> {
        let size = terminal.size()?;
        let (shell_cols, shell_rows) = calculate_shell_size(size.width, size.height);
        Ok(Self {
            shell: ShellState::new(shell_cols, shell_rows)?,
            term_display: TerminalDisplay::new(shell_cols, shell_rows),
            debug_panel: DebugPanel::new(),
            exit: false,
            active_panel: ActivePanel::Terminal,
            input_mode: InputMode::Normal,
        })
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut last_draw = std::time::Instant::now();
        let mut last_input_log = std::time::Instant::now();
        let mut last_output_log = std::time::Instant::now();
        // Output at 32FPS when there is nothing active (input, output, ...)
        let frame_time: Duration = Duration::from_millis(1000/32);
        // Output at 125FPS when there is something active.
        let min_frame_time: Duration = Duration::from_millis(1000/125);

        // Adaptive batch processing
        // If there are too many output bytesfrom the pty,
        // we have to do more work in a single iteration.
        let mut consecutive_output_frames = 0;

        self.debug_panel.add_log("Event loop started");

        while !self.exit {
            self.debug_panel.increase_loop_count(1);
            let loop_start = std::time::Instant::now();

            // PRIORITY 1: Check for user input first (ensures responsiveness)
            let has_input = event::poll(Duration::from_millis(1))?;
            if has_input {
                self.debug_panel.set_last_input_time(std::time::Instant::now());
                if loop_start.duration_since(last_input_log) > Duration::from_millis(100) {
                    self.debug_panel.add_log("Input detected");
                    last_input_log = loop_start;
                }
                self.handle_events()?;
            }

            // PRIORITY 2: Process shell output (with adaptive batching)
            // Normal load: process up to 800KB per loop
            // High load: process up to 2.4MB per loop to maximize throughput
            let max_bytes = if consecutive_output_frames > 5 {
                2400 * 1024  // 2.4MB for high throughput
            } else {
                800 * 1024   // 800KB for normal operation
            };

            let (has_output, filtered_data, query_logs) = self.shell.read_output_debug(
                max_bytes,
                || self.term_display.cursor_position()
            );

            // Process output data
            if !filtered_data.is_empty() {
                self.term_display.process(&filtered_data);
            }

            // Log queries (these are rare, so always log them)
            for log in query_logs {
                self.debug_panel.add_log(log);
            }

            let total_bytes = filtered_data.len();

            // Track consecutive output frames for adaptive processing
            if has_output {
                consecutive_output_frames += 1;
            } else {
                consecutive_output_frames = 0;
            }

            // Log output only occasionally to avoid spam
            if has_output && loop_start.duration_since(last_output_log) > Duration::from_millis(500) {
                self.debug_panel.add_log(format!("Shell output: {} bytes (load: {})",
                    total_bytes, consecutive_output_frames));
                last_output_log = loop_start;
            }

            // Check if shell has exited
            if self.shell.is_shell_exited() {
                self.debug_panel.add_log("Shell exited".to_string());
                self.exit = true;
                break;
            }

            // PRIORITY 3: Render (rate-limited, but responsive)
            // Balance between responsiveness and throughput
            let should_draw = if has_input {
                // User input: always render immediately for responsive feel
                true
            } else if has_output {
                // Output: render at fixed intervals to maintain throughput
                loop_start - last_draw >= min_frame_time
            } else {
                // No activity: check if enough time passed for periodic refresh
                loop_start - last_draw >= frame_time
            };

            if should_draw {
                terminal.draw(|frame| self.draw(frame))?;
                self.debug_panel.record_frame();
                last_draw = loop_start;
            }

            // Small sleep to prevent busy-waiting and reduce CPU usage
            // Under high load, minimal sleep to maximize throughput
            if has_input || has_output {
                // there is no time to sleep!
            } else {
                // take a break
                let now = std::time::Instant::now();
                let elapsed_since_loop = now.duration_since(loop_start);
                let remaining = min_frame_time.saturating_sub(elapsed_since_loop);

                // Sleep for the remaining time, but leave a small margin
                if remaining > Duration::from_millis(1) {
                    thread::sleep(remaining.saturating_sub(Duration::from_micros(1000)));
                }
            }
        }
        Ok(())
    }

    /// Renders the application to the given frame.
    ///
    /// # Arguments
    /// * `frame` - The frame to render to
    fn draw(&self, frame: &mut Frame) {
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

    /// Calculates the screen cursor position from the terminal cursor position.
    ///
    /// # Arguments
    /// * `area` - The render area for the application
    ///
    /// # Returns
    /// The (x, y) cursor position in screen coordinates, or None if cursor is not visible
    fn get_cursor_position(&self, area: Rect) -> Option<(u16, u16)> {
        // Get cursor position from terminal display
        let cursor = self.term_display.cursor_position();

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
        let cursor_x = inner_area.x + cursor.1;
        let cursor_y = inner_area.y + cursor.0;

        // Make sure cursor is within the shell area
        if cursor_x < inner_area.x + shell_width && cursor_y < inner_area.y + inner_area.height {
            Some((cursor_x, cursor_y))
        } else {
            None
        }
    }

    /// Updates the application's state based on user input.
    /// Handles keyboard, mouse, and resize events.
    fn handle_events(&mut self) -> Result<()> {
        if !event::poll(Duration::ZERO)? {
            return Ok(());
        }
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                // Log the key event details
                let event_desc = format_key_event(&key_event);
                self.debug_panel.add_log(format!("Key: {}", event_desc));
                self.handle_key_event(key_event)?;
            }
            Event::Resize(cols, rows) => {
                self.debug_panel.add_log(format!("Resize: {}x{}", cols, rows));
                // Calculate the actual shell area size (left side only)
                let (shell_cols, shell_rows) = calculate_shell_size(cols, rows);
                // Update PTY size and terminal display when resized
                self.shell.resize(shell_cols, shell_rows)?;
                self.term_display.resize(shell_cols, shell_rows);
            }
            Event::Mouse(mouse_event) => {
                use crossterm::event::MouseEventKind;
                self.debug_panel.add_log(format!("Mouse: {:?}", mouse_event));

                let (cols, rows) = crossterm::terminal::size()?;
                let frame_size = Rect::new(0, 0, cols, rows);
                let mouse_panel = self.get_panel_at_position(mouse_event.column, mouse_event.row, frame_size);

                // Check if we should forward mouse event to shell
                // Forward if:
                // 1. Mouse is over Terminal panel AND
                // 2. Terminal is in normal mode (not browsing) AND
                // 3. The program in shell has enabled mouse mode (e.g., vim with :set mouse=a)
                let should_forward = matches!(mouse_panel, Some(ActivePanel::Terminal))
                    && self.input_mode == InputMode::Normal
                    && self.shell.is_mouse_mode_enabled();

                if should_forward {
                    // Forward mouse event to shell (for programs like vim, tmux, etc.)
                    self.shell.send_mouse(mouse_event)?;
                    self.debug_panel.add_log("→ Mouse forwarded to shell".to_string());

                    // Update active panel if needed
                    if self.active_panel != ActivePanel::Terminal {
                        self.active_panel = ActivePanel::Terminal;
                    }
                } else {
                    // Handle mouse event in TUI (panel switching, scrolling)
                    match mouse_event.kind {
                        MouseEventKind::ScrollUp => {
                            // Mouse scroll up: set focus to panel under mouse, scroll, and enter browsing mode
                            if let Some(panel) = mouse_panel {
                                self.active_panel = panel;
                            }
                            self.scroll_active_panel_up(3);
                            self.input_mode = InputMode::Browsing;
                            self.debug_panel.add_log(format!("→ Enter browsing mode, scroll up (offset: {})", self.get_active_panel_offset()));
                        }
                        MouseEventKind::ScrollDown => {
                            // Mouse scroll down: set focus to panel under mouse, scroll
                            if let Some(panel) = mouse_panel {
                                self.active_panel = panel;
                            }
                            self.scroll_active_panel_down(3);
                            // Only enter browsing mode if we're actually scrolled back
                            let is_scrolled = match self.active_panel {
                                ActivePanel::Terminal => self.term_display.is_scrolled(),
                                ActivePanel::DebugPanel => self.debug_panel.is_scrolled(),
                            };
                            if is_scrolled {
                                self.input_mode = InputMode::Browsing;
                                self.debug_panel.add_log(format!("→ Scroll down (offset: {})", self.get_active_panel_offset()));
                            } else {
                                // Scrolled to bottom, return to normal mode
                                self.input_mode = InputMode::Normal;
                                self.debug_panel.add_log("→ Scroll to bottom, exit browsing mode".to_string());
                            }
                        }
                        MouseEventKind::Down(_button) => {
                            // Mouse click: switch focus to panel under mouse
                            if let Some(panel) = mouse_panel {
                                self.active_panel = panel;
                                self.debug_panel.add_log(format!("→ Click: switch to {:?}", panel));
                            }
                        }
                        _ => {
                            // Other mouse events: ignore for now
                        }
                    }
                }
            }
            _ => {}
        };
        Ok(())
    }

    /// Handles keyboard input events based on the current input mode.
    ///
    /// # Arguments
    /// * `key_event` - The keyboard event to handle
    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        match self.input_mode {
            InputMode::Normal => {
                // Check if this is the leader key (Ctrl+] or Ctrl+5)
                // Note: Due to crossterm's parsing, both Ctrl+] and Ctrl+5 produce the same byte (0x1D)
                // and crossterm reports it as Ctrl+5. We accept both to handle this correctly.
                if key_event.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key_event.code, KeyCode::Char(']') | KeyCode::Char('5')) {
                    self.debug_panel.add_log("Action: Enter leader mode".to_string());
                    self.input_mode = InputMode::LeaderCommand;
                } else {
                    // Normal mode: only send keys to shell if Terminal is active
                    match self.active_panel {
                        ActivePanel::Terminal => {
                            // Terminal active: send keys to shell
                            self.shell.send_key(key_event)?;
                        }
                        ActivePanel::DebugPanel => {
                            // DebugPanel active: ignore input (it's read-only)
                            // User can use Ctrl+] to switch panels or enter browsing mode
                            self.debug_panel.add_log("Key ignored (DebugPanel is read-only)".to_string());
                        }
                    }
                }
            }
            InputMode::LeaderCommand => {
                self.debug_panel.add_log("Action: Leader command".to_string());
                self.input_mode = InputMode::Normal;  // Reset to normal by default
                self.handle_leader_command(key_event)?;
            }
            InputMode::Browsing => {
                self.handle_browsing_mode(key_event)?;
            }
        }
        Ok(())
    }

    /// Handles commands in leader command mode (triggered by Ctrl+]).
    ///
    /// # Arguments
    /// * `key_event` - The keyboard event to handle as a leader command
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
            KeyCode::Left => {
                // Ctrl+] Left: Switch to Terminal panel
                self.active_panel = ActivePanel::Terminal;
                self.debug_panel.add_log("Switch to Terminal panel".to_string());
            }
            KeyCode::Right => {
                // Ctrl+] Right: Switch to Debug panel
                self.active_panel = ActivePanel::DebugPanel;
                self.debug_panel.add_log("Switch to Debug panel".to_string());
            }
            KeyCode::Up => {
                // Ctrl+] Up: Scroll up in active panel and enter browsing mode
                self.scroll_active_panel_up(1);
                self.input_mode = InputMode::Browsing;
                self.debug_panel.add_log(format!("Enter browsing mode, scroll up (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::Down => {
                // Ctrl+] Down: Scroll down in active panel and enter browsing mode
                self.scroll_active_panel_down(1);
                self.input_mode = InputMode::Browsing;
                self.debug_panel.add_log(format!("Enter browsing mode, scroll down (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::PageUp => {
                // Ctrl+] PageUp: Scroll up by page in active panel and enter browsing mode
                self.scroll_active_panel_up(10);
                self.input_mode = InputMode::Browsing;
                self.debug_panel.add_log(format!("Enter browsing mode, scroll page up (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::PageDown => {
                // Ctrl+] PageDown: Scroll down by page in active panel and enter browsing mode
                self.scroll_active_panel_down(10);
                self.input_mode = InputMode::Browsing;
                self.debug_panel.add_log(format!("Enter browsing mode, scroll page down (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::Home => {
                // Ctrl+] Home: Scroll to top of active panel and enter browsing mode
                self.scroll_active_panel_up(usize::MAX);
                self.input_mode = InputMode::Browsing;
                self.debug_panel.add_log(format!("Enter browsing mode, scroll to top (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::End => {
                // Ctrl+] End: Scroll to bottom of active panel
                self.scroll_active_panel_to_bottom();
                self.debug_panel.add_log("Scroll to bottom".to_string());
            }
            KeyCode::Esc => {
                // ESC: Cancel leader mode (already reset in handle_key_event)
            }
            _ => {
                // Unknown command - just cancel (do nothing)
                // Mode was already reset to Normal in handle_key_event
            }
        }
        Ok(())
    }

    /// Handles keyboard input in browsing mode (for scrolling through output).
    ///
    /// # Arguments
    /// * `key_event` - The keyboard event to handle in browsing mode
    fn handle_browsing_mode(&mut self, key_event: KeyEvent) -> Result<()> {
        match key_event.code {
            KeyCode::Up => {
                // Continue scrolling up in active panel
                self.scroll_active_panel_up(1);
                self.debug_panel.add_log(format!("Scroll up (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::Down => {
                // Continue scrolling down in active panel
                self.scroll_active_panel_down(1);
                self.debug_panel.add_log(format!("Scroll down (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::PageUp => {
                // Continue scrolling up by page in active panel
                self.scroll_active_panel_up(10);
                self.debug_panel.add_log(format!("Scroll page up (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::PageDown => {
                // Continue scrolling down by page in active panel
                self.scroll_active_panel_down(10);
                self.debug_panel.add_log(format!("Scroll page down (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::Home => {
                // Scroll to top of active panel
                self.scroll_active_panel_up(usize::MAX);
                self.debug_panel.add_log(format!("Scroll to top (offset: {})", self.get_active_panel_offset()));
            }
            KeyCode::End => {
                // Scroll to bottom of active panel and exit browsing mode
                self.scroll_active_panel_to_bottom();
                self.input_mode = InputMode::Normal;
                self.debug_panel.add_log("Exit browsing mode, scroll to bottom".to_string());
            }
            KeyCode::Left => {
                // Switch to Terminal panel (stay in browsing mode)
                self.active_panel = ActivePanel::Terminal;
                self.debug_panel.add_log("Switch to Terminal panel".to_string());
            }
            KeyCode::Right => {
                // Switch to Debug panel (stay in browsing mode)
                self.active_panel = ActivePanel::DebugPanel;
                self.debug_panel.add_log("Switch to Debug panel".to_string());
            }
            KeyCode::Esc => {
                // ESC: Exit browsing mode and return to bottom
                self.scroll_active_panel_to_bottom();
                self.active_panel = ActivePanel::Terminal;
                self.input_mode = InputMode::Normal;
                self.debug_panel.add_log("Exit browsing mode (ESC)".to_string());
            }
            _ => {
                // Any other key: exit browsing mode, switch to terminal, return to bottom, and send key to shell
                self.term_display.scroll_to_bottom();
                self.debug_panel.scroll_to_bottom();
                self.active_panel = ActivePanel::Terminal;
                self.input_mode = InputMode::Normal;
                self.debug_panel.add_log("Exit browsing mode (input)".to_string());
                // Send the key to shell
                self.shell.send_key(key_event)?;
            }
        }
        Ok(())
    }

    /// Scrolls the active panel up by n lines.
    ///
    /// # Arguments
    /// * `n` - Number of lines to scroll up
    fn scroll_active_panel_up(&mut self, n: usize) {
        match self.active_panel {
            ActivePanel::Terminal => self.term_display.scroll_up(n),
            ActivePanel::DebugPanel => self.debug_panel.scroll_up(n),
        }
    }

    /// Scrolls the active panel down by n lines.
    ///
    /// # Arguments
    /// * `n` - Number of lines to scroll down
    fn scroll_active_panel_down(&mut self, n: usize) {
        match self.active_panel {
            ActivePanel::Terminal => self.term_display.scroll_down(n),
            ActivePanel::DebugPanel => self.debug_panel.scroll_down(n),
        }
    }

    /// Scrolls the active panel to the bottom (latest content).
    fn scroll_active_panel_to_bottom(&mut self) {
        match self.active_panel {
            ActivePanel::Terminal => self.term_display.scroll_to_bottom(),
            ActivePanel::DebugPanel => self.debug_panel.scroll_to_bottom(),
        }
    }

    /// Gets the scroll offset of the active panel.
    ///
    /// # Returns
    /// The number of lines scrolled back from the bottom
    fn get_active_panel_offset(&self) -> usize {
        match self.active_panel {
            ActivePanel::Terminal => self.term_display.scroll_offset(),
            ActivePanel::DebugPanel => self.debug_panel.scroll_offset(),
        }
    }

    /// Determines which panel is at the given mouse position.
    ///
    /// # Arguments
    /// * `col` - The mouse column position
    /// * `row` - The mouse row position
    /// * `frame_size` - The size of the frame
    ///
    /// # Returns
    /// The panel at the given position, or None if not over a panel
    fn get_panel_at_position(&self, col: u16, row: u16, frame_size: Rect) -> Option<ActivePanel> {
        // Calculate layout similar to render logic
        // Skip top border (1 row)
        if row < 1 || row >= frame_size.height.saturating_sub(1) {
            return None;  // In border area
        }

        let inner_width = frame_size.width;
        let shell_width = inner_width.saturating_sub(SIDEBAR_WIDTH);

        if col < shell_width {
            Some(ActivePanel::Terminal)
        } else if col >= shell_width && col < inner_width {
            Some(ActivePanel::DebugPanel)
        } else {
            None
        }
    }

    /// Marks the application for exit.
    fn exit(&mut self) {
        self.exit = true;
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Render the outer block with top and bottom borders
        let (top_title, bottom_title) = match self.input_mode {
            InputMode::Browsing => {
                // In browsing mode, show active panel and scroll commands
                let panel_name = match self.active_panel {
                    ActivePanel::Terminal => "Terminal",
                    ActivePanel::DebugPanel => "Debug",
                };
                let offset = self.get_active_panel_offset();
                let top_text = format!(" RustyTerm [BROWSING: {} -{} lines] ", panel_name, offset);
                let top = Line::from(top_text.bold())
                    .style(Style::new().bg(Color::Cyan).fg(Color::Black));
                let bottom = Line::from(" Navigate(↑↓) | Switch(←→) | Page(PgUp/PgDn) | Top/Bottom(Home/End) | Exit(ESC/any key) ")
                    .style(Style::new().bg(Color::Cyan).fg(Color::Black));
                (top, bottom)
            }
            InputMode::LeaderCommand => {
                // Leader key was pressed, show available commands
                let panel_indicator = match self.active_panel {
                    ActivePanel::Terminal => "[Terminal]",
                    ActivePanel::DebugPanel => "[Debug]",
                };
                let top_text = format!(" RustyTerm [COMMAND MODE] {} ", panel_indicator);
                let top = Line::from(top_text.bold())
                    .style(Style::new().bg(Color::Yellow).fg(Color::Black));
                let bottom = Line::from(" Quit(q) | Switch(←→) | Scroll(↑↓) | Send ^](^])| Cancel(ESC) ")
                    .style(Style::new().bg(Color::Yellow).fg(Color::Black));
                (top, bottom)
            }
            InputMode::Normal => {
                // Normal mode, show active panel indicator
                let panel_indicator = match self.active_panel {
                    ActivePanel::Terminal => "Terminal",
                    ActivePanel::DebugPanel => "Debug",
                };
                let top_text = format!(" RustyTerm [{}] ", panel_indicator);
                let top = Line::from(top_text.bold())
                    .style(Style::new().bg(Color::DarkGray));
                let bottom = Line::from(" Commands(^]) | Switch(Click/Mouse) ")
                    .style(Style::new().bg(Color::DarkGray));
                (top, bottom)
            }
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
        let is_terminal_active = self.active_panel == ActivePanel::Terminal;
        self.render_shell(chunks[0], buf, is_terminal_active);

        // Render right side: sidebar (with special left border)
        let is_debug_active = self.active_panel == ActivePanel::DebugPanel;
        self.render_sidebar(chunks[1], buf, is_debug_active);
    }
}

impl App {
    /// Renders the shell terminal panel.
    ///
    /// # Arguments
    /// * `area` - The area to render to
    /// * `buf` - The buffer to render into
    /// * `is_active` - Whether this panel is currently active
    fn render_shell(&self, area: Rect, buf: &mut Buffer, is_active: bool) {
        // Get terminal output as styled lines
        let terminal_output = self.term_display.get_lines();

        // Optionally add a subtle background highlight for active panel
        let paragraph = if is_active && self.input_mode == InputMode::Browsing {
            // Active in browsing mode: subtle highlight
            Paragraph::new(terminal_output)
                .style(Style::default().bg(Color::Rgb(20, 20, 30)))
        } else {
            // Normal rendering
            Paragraph::new(terminal_output)
        };

        paragraph.render(area, buf);
    }

    /// Renders the debug sidebar panel.
    ///
    /// # Arguments
    /// * `area` - The area to render to
    /// * `buf` - The buffer to render into
    /// * `is_active` - Whether this panel is currently active
    fn render_sidebar(&self, area: Rect, buf: &mut Buffer, is_active: bool) {
        let shell_stats = ShellStats {
            total_bytes_received: self.shell.total_bytes_received,
            last_output_time: self.shell.last_output_time,
        };

        self.debug_panel.render(
            area,
            buf,
            &shell_stats,
            is_active,
            self.input_mode == InputMode::Browsing,
        );
    }
}


// ============================================================================
// Helper Functions
// ============================================================================

/// Formats a key event as a human-readable string for debugging.
///
/// # Arguments
/// * `key_event` - The key event to format
///
/// # Returns
/// A string representation of the key event
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

/// Calculates the actual shell area size based on terminal dimensions.
///
/// This accounts for:
/// - Top and bottom borders (2 rows)
/// - Right sidebar width (configured by SIDEBAR_WIDTH constant)
///
/// # Arguments
/// * `terminal_cols` - The total terminal width in columns
/// * `terminal_rows` - The total terminal height in rows
///
/// # Returns
/// A tuple of (columns, rows) for the shell area
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
