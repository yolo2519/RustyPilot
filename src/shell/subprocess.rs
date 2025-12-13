//! Shell subprocess management and command execution.
//!
//! This module manages the shell subprocess lifecycle, sends commands
//! to the shell, and reads output for display in the UI.

use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc::{self, Receiver, UnboundedSender};
use tracing::error;

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

        // Create channel for PTY output
        let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(PTY_OUTPUT_BUFFER);

        // Clone event sink for reader thread
        let event_sink_clone = event_sink.clone();

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
            },
            output_rx,
        ))
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
}
