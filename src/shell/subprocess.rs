//! Shell subprocess management and command execution.
//!
//! This module manages the shell subprocess lifecycle, sends commands
//! to the shell, and reads output for display in the UI.

use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::{Context, Result};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc::{self, Receiver, UnboundedSender};
use tracing::error;

use crate::context::CommandLog;
use crate::event::AppEvent;

// Channel buffer sizes
const PTY_OUTPUT_BUFFER: usize = 1024;  // Can buffer ~1-5MB data for smooth rendering
const PTY_READ_BUFFER: usize = 16384;   // 16KB per read for good throughput

/// Manages the shell subprocess using a PTY.
pub struct ShellManager {
    #[allow(unused)]
    event_sink: UnboundedSender<AppEvent>,
    pty_master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pty_writer: Arc<Mutex<Box<dyn Write + Send>>>,
    command_log: Arc<Mutex<CommandLog>>,
}

impl ShellManager {
    /// Creates a new shell manager with a PTY of the specified size.
    ///
    /// # Arguments
    /// * `event_sink` - Channel for sending app events (e.g., shell errors)
    /// * `cols` - Terminal width in columns
    /// * `rows` - Terminal height in rows
    ///
    /// # Returns
    /// A tuple of (ShellManager, Receiver for PTY output)
    pub fn new(
        event_sink: UnboundedSender<AppEvent>,
        cols: u16,
        rows: u16,
    ) -> Result<(Self, Receiver<Vec<u8>>)> {
        let pty_system = native_pty_system();

        // Create PTY with specified size
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Spawn shell from $SHELL environment variable, fallback to default
        let shell_cmd = std::env::var("SHELL").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
                "/bin/zsh".to_string()
            } else {
                "/bin/bash".to_string()
            }
        });

        let mut cmd = CommandBuilder::new(&shell_cmd);
        cmd.env("TERM", "xterm-256color");

        // Inherit current working directory
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        let _child = pair.slave.spawn_command(cmd)?;

        // Drop slave side in parent process
        drop(pair.slave);

        // Note: We don't need to keep _child alive because the PTY will remain open
        // as long as pty_master exists. The child process will exit when the PTY closes.

        let reader = pair.master.try_clone_reader()?;
        let pty_writer = pair.master.take_writer()?;
        let pty_master = Arc::new(Mutex::new(pair.master));
        let pty_writer = Arc::new(Mutex::new(pty_writer));

        // Create command log with max 200 entries
        let command_log = Arc::new(Mutex::new(CommandLog::new(200)));

        // Create channel for PTY output
        let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(PTY_OUTPUT_BUFFER);

        // Clone event sink for reader thread
        let event_sink_clone = event_sink.clone();
        
        // Clone command log for reader thread
        let command_log_clone = command_log.clone();

        // Spawn reader thread
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; PTY_READ_BUFFER];

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF: shell exited
                        if let Err(e) = event_sink_clone.send(AppEvent::ShellError {
                            message: "Shell process exited".to_string(),
                        }) {
                            error!("Failed to send ShellError event (shell exited): {:?}", e);
                        }
                        break;
                    }
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        
                        // Append output to current command in log
                        if let Ok(mut log) = command_log_clone.lock() {
                            log.append_output(&data);
                        }
                        
                        // Use blocking_send since we're in a std::thread
                        if output_tx.blocking_send(data).is_err() {
                            // Receiver dropped, exit thread
                            break;
                        }
                    }
                    Err(e) => {
                        // IO error
                        if e.kind() != std::io::ErrorKind::Interrupted {
                            if let Err(send_err) = event_sink_clone.send(AppEvent::ShellError {
                                message: format!("PTY read error: {}", e),
                            }) {
                                error!("Failed to send ShellError event (PTY read error): {:?}", send_err);
                            }
                            break;
                        }
                        // Interrupted errors are non-fatal, continue
                    }
                }
            }
        });

        Ok((
            Self {
                event_sink,
                pty_master,
                pty_writer,
                command_log,
            },
            output_rx,
        ))
    }

    /// Start recording a new command in the log.
    ///
    /// This should be called when the user presses Enter to execute a command.
    pub fn start_new_command(&mut self, command_line: String) {
        if let Ok(mut log) = self.command_log.lock() {
            log.start_new_command(command_line);
        }
    }

    /// Get recent command records for context.
    ///
    /// Returns up to `limit` most recent commands with their outputs.
    pub fn recent_command_records(&self, limit: usize) -> Vec<crate::context::CommandRecord> {
        if let Ok(log) = self.command_log.lock() {
            log.recent(limit).to_vec()
        } else {
            Vec::new()
        }
    }

    /// Handles user input by writing it to the PTY.
    ///
    /// # Arguments
    /// * `data` - Raw bytes to send to the shell
    pub fn handle_user_input(&mut self, data: &[u8]) -> Result<()> {
        let mut pty_writer = self.pty_writer.lock().map_err(|e| {
            anyhow::anyhow!("Failed to lock PTY writer: {}", e)
        })?;

        pty_writer.write_all(data)?;
        pty_writer.flush()?;
        Ok(())
    }

    /// Injects a command into the shell as if the user typed it and pressed Enter.
    ///
    /// # Arguments
    /// * `cmd` - Command string to execute
    pub fn inject_command(&mut self, cmd: &str) -> Result<()> {
        let mut pty_writer = self.pty_writer.lock().map_err(|e| {
            anyhow::anyhow!("Failed to lock PTY writer: {}", e)
        })?;

        // Write command
        pty_writer.write_all(cmd.as_bytes())?;
        // Add newline to execute
        pty_writer.write_all(b"\n")?;
        pty_writer.flush()?;

        Ok(())
    }

    /// Executes a command visibly in the shell, as if the user typed it.
    ///
    /// This method writes the command string followed by a newline to the PTY,
    /// making it appear in the terminal as if the user had manually entered it.
    /// The command will be visible in the shell's history and output.
    ///
    /// # Non-blocking Behavior
    /// This method returns immediately after writing the command to the PTY.
    /// It does NOT wait for the command to complete. Output from the command
    /// will appear asynchronously through the existing PTY read loop and will
    /// be delivered via the output channel created in `ShellManager::new()`.
    ///
    /// # Arguments
    /// * `cmd` - The command string to execute (without trailing newline)
    ///
    /// # Returns
    /// * `Ok(())` if the command was successfully written to the PTY
    /// * `Err(_)` if the PTY writer lock could not be acquired or write failed
    ///
    /// # Example
    /// ```no_run
    /// # use rusty_term::shell::ShellManager;
    /// # use tokio::sync::mpsc::unbounded_channel;
    /// # fn example() -> anyhow::Result<()> {
    /// # let (tx, _) = unbounded_channel();
    /// # let (mut shell, _rx) = ShellManager::new(tx, 80, 24)?;
    /// // Execute a simple command
    /// shell.execute_visible("ls -la")?;
    ///
    /// // Execute a git command
    /// shell.execute_visible("git status")?;
    ///
    /// // The output will appear asynchronously via the output receiver
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// Returns an error if:
    /// - The PTY writer mutex is poisoned
    /// - Writing to the PTY fails (e.g., shell process has exited)
    /// - Flushing the PTY writer fails
    pub fn execute_visible(&mut self, cmd: &str) -> Result<()> {
        let mut pty_writer = self
            .pty_writer
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock PTY writer: {}", e))
            .context("Unable to acquire PTY writer lock for command execution")?;

        // Write command string to PTY
        pty_writer
            .write_all(cmd.as_bytes())
            .context("Failed to write command to PTY")?;

        // Add newline to execute the command
        pty_writer
            .write_all(b"\n")
            .context("Failed to write newline to PTY")?;

        // Flush to ensure command is sent immediately
        pty_writer
            .flush()
            .context("Failed to flush PTY writer")?;

        Ok(())
    }

    /// Resizes the PTY to the specified dimensions.
    ///
    /// # Arguments
    /// * `cols` - New terminal width in columns
    /// * `rows` - New terminal height in rows
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        let pty = self.pty_master.lock().map_err(|e| {
            anyhow::anyhow!("Failed to lock PTY master: {}", e)
        })?;

        pty.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        Ok(())
    }

    /// Sends a mouse event to the shell using SGR extended mouse protocol.
    ///
    /// This converts the crossterm MouseEvent to the SGR mouse protocol format
    /// (ESC[<Cb;Cx;CyM or m) and sends it to the PTY.
    ///
    /// # Arguments
    /// * `event` - The mouse event to send
    /// * `terminal_area` - The terminal's inner area for coordinate translation
    pub fn send_mouse(&mut self, event: MouseEvent, terminal_area: ratatui::layout::Rect) -> Result<()> {
        let bytes = mouse_to_sgr_bytes(event, terminal_area);
        if !bytes.is_empty() {
            self.handle_user_input(&bytes)?;
        }
        Ok(())
    }
}

/// Convert crossterm MouseEvent to SGR mouse protocol bytes.
///
/// SGR mouse protocol format: ESC [ < Cb ; Cx ; Cy M (press) or m (release)
///
/// Button codes:
/// - 0=left, 1=middle, 2=right
/// - 32=motion (drag with left), 33=motion (drag with middle), 34=motion (drag with right)
/// - 64=wheel up, 65=wheel down
///
/// Modifier additions:
/// - +4 for Shift, +8 for Alt, +16 for Ctrl
///
/// Reference: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Extended-coordinates
fn mouse_to_sgr_bytes(event: MouseEvent, terminal_area: ratatui::layout::Rect) -> Vec<u8> {
    let (mut button_code, is_release) = match event.kind {
        MouseEventKind::Down(MouseButton::Left) => (0, false),
        MouseEventKind::Down(MouseButton::Middle) => (1, false),
        MouseEventKind::Down(MouseButton::Right) => (2, false),
        MouseEventKind::Up(MouseButton::Left) => (0, true),
        MouseEventKind::Up(MouseButton::Middle) => (1, true),
        MouseEventKind::Up(MouseButton::Right) => (2, true),
        MouseEventKind::Drag(MouseButton::Left) => (32, false),
        MouseEventKind::Drag(MouseButton::Middle) => (33, false),
        MouseEventKind::Drag(MouseButton::Right) => (34, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        MouseEventKind::Moved => return vec![], // Ignore move without button
        _ => return vec![],
    };

    // Add modifier keys to button code
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        button_code += 4;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        button_code += 8;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        button_code += 16;
    }

    // Convert screen coordinates to terminal-relative coordinates (1-based)
    // The event coordinates are absolute screen positions, we need to translate
    // them to be relative to the terminal area
    let x = event.column.saturating_sub(terminal_area.x) + 1;
    let y = event.row.saturating_sub(terminal_area.y) + 1;

    // SGR mouse protocol: ESC [ < Cb ; Cx ; Cy M/m
    let terminator = if is_release { 'm' } else { 'M' };
    format!("\x1b[<{};{};{}{}", button_code, x, y, terminator).into_bytes()
}
