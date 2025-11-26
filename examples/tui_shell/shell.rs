//! Shell state management and PTY interaction.

use std::sync::mpsc;
use std::io::Write;
use std::thread;
use std::io;

use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use portable_pty::MasterPty;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Manages the shell process, PTY, and I/O communication.
pub struct ShellState {
    writer: Box<dyn Write + Send>,
    output_rx: mpsc::Receiver<Vec<u8>>,
    pty_master: Box<dyn MasterPty + Send>,
    shell_exited: bool,  // Track if the shell process has exited
    pub last_output_time: Option<std::time::Instant>,  // Debug: track when last output arrived
    pub total_bytes_received: usize,  // Debug: total bytes received from shell
    mouse_mode_enabled: bool,  // Track if the program has enabled mouse reporting
}

impl ShellState {
    /// Creates a new shell state with a PTY of the specified size.
    ///
    /// # Arguments
    /// * `cols` - Terminal width in columns
    /// * `rows` - Terminal height in rows
    ///
    /// # Returns
    /// A new `ShellState` instance with a running shell process
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        let pty_system = native_pty_system();

        // Create a new PTY
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Spawn the shell that the user uses in the PTY
        let shell_cmd = std::env::var("SHELL")?;
        let mut cmd = CommandBuilder::new(shell_cmd);
        cmd.env("TERM", "xterm-256color");
        cmd.cwd(std::env::current_dir()?);  // Inherit the current working directory
        let _child = pair.slave.spawn_command(cmd)?;

        // Drop the slave side in the parent process
        drop(pair.slave);

        let writer = pair.master.take_writer()?;
        let reader = pair.master.try_clone_reader()?;
        let pty_master = pair.master;

        // Create bounded mpsc channel for output (with backpressure)
        // Increased buffer for better throughput (1000 chunks = ~4MB max)
        // This balances between memory usage and performance
        let (output_tx, output_rx) = mpsc::sync_channel::<Vec<u8>>(1000);

        // Spawn reader thread with larger buffer for better throughput
        thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 16384];

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
                        // sync_channel will block here if channel is full (backpressure)
                        if output_tx.send(data).is_err() {
                            // Main thread dropped the receiver, exit
                            break;
                        }
                    }
                    Err(e) => {
                        // Handle errors - for now just log and continue
                        // TODO(not for now): do we need to change this?
                        eprintln!("PTY read error: {}", e);
                        if e.kind() != io::ErrorKind::Interrupted {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            writer,
            output_rx,
            pty_master,
            shell_exited: false,
            last_output_time: None,
            total_bytes_received: 0,
            mouse_mode_enabled: false,
        })
    }

    /// Reads and processes output from the shell with debugging information.
    ///
    /// # Arguments
    /// * `max_bytes` - Maximum bytes to process in one call
    /// * `get_cursor` - Closure to get the current cursor position for query responses
    ///
    /// # Returns
    /// A tuple of (has_output, filtered_data, query_logs):
    /// - has_output: Whether any output was received
    /// - filtered_data: Output data with terminal queries filtered out
    /// - query_logs: Debug log messages about terminal queries
    pub fn read_output_debug(&mut self, max_bytes: usize, get_cursor: impl Fn() -> (u16, u16)) -> (bool, Vec<u8>, Vec<String>) {
        // Non-blocking: read available data from the channel with batching limit
        let mut has_output = false;
        let mut all_filtered_data = Vec::new();
        let mut query_logs = Vec::new();
        let mut bytes_processed = 0;

        while let Ok(data) = self.output_rx.try_recv() {
            if data.is_empty() {
                // Empty vec is EOF marker - shell has exited
                self.shell_exited = true;
                has_output = true;
                break;
            }

            bytes_processed += data.len();
            self.total_bytes_received += data.len();
            self.last_output_time = Some(std::time::Instant::now());

            // Detect mouse mode changes (programs like vim enabling/disabling mouse)
            self.detect_mouse_mode_changes(&data);

            // Check for and respond to terminal queries, get filtered data
            let (filtered_data, log) = self.handle_terminal_queries(&data, &get_cursor);
            if let Some(log_msg) = log {
                query_logs.push(log_msg);
            }

            // Collect filtered output (queries removed)
            if !filtered_data.is_empty() {
                all_filtered_data.extend_from_slice(&filtered_data);
                has_output = true;
            } else if !query_logs.is_empty() {
                // We had queries but no display data
                has_output = true;
            }

            // Limit processing to prevent blocking on high-throughput commands like `yes`
            if bytes_processed >= max_bytes {
                break;
            }
        }
        (has_output, all_filtered_data, query_logs)
    }

    /// Handles terminal query sequences (CPR, DA) by responding to them and filtering them out.
    ///
    /// # Arguments
    /// * `data` - The raw data from the PTY
    /// * `get_cursor` - Closure to get the current cursor position
    ///
    /// # Returns
    /// A tuple of (filtered_data, log_msg):
    /// - filtered_data: Data with queries removed
    /// - log_msg: Optional debug log message about the query
    fn handle_terminal_queries(&mut self, data: &[u8], get_cursor: &impl Fn() -> (u16, u16)) -> (Vec<u8>, Option<String>) {
        // Look for common terminal query sequences and filter them out
        // CPR (Cursor Position Report): ESC[6n
        // DA (Device Attributes): ESC[c or ESC[>c

        let mut data_str = String::from_utf8_lossy(data).to_string();
        let mut log_msg = None;

        // Check for queries and respond, then filter them out

        // ESC[6n - Cursor Position Report query
        if data_str.contains("\x1b[6n") {
            let cursor = get_cursor();
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

    /// Checks if the shell process has exited.
    ///
    /// # Returns
    /// True if the shell has exited, false otherwise
    pub fn is_shell_exited(&self) -> bool {
        self.shell_exited
    }

    /// Checks if the program running in the shell has enabled mouse mode.
    ///
    /// # Returns
    /// True if mouse mode is enabled (e.g., vim with mouse support), false otherwise
    pub fn is_mouse_mode_enabled(&self) -> bool {
        self.mouse_mode_enabled
    }

    /// Detects mouse mode control sequences and updates the mouse mode state.
    ///
    /// # Arguments
    /// * `data` - The raw data from the PTY to scan for mouse mode sequences
    fn detect_mouse_mode_changes(&mut self, data: &[u8]) {
        // Detect mouse mode control sequences from programs (e.g., vim)
        // Common sequences:
        // ESC[?1000h - Enable basic mouse tracking
        // ESC[?1002h - Enable button event tracking
        // ESC[?1003h - Enable any event tracking
        // ESC[?1006h - Enable SGR extended mouse mode
        // ESC[?...l - Disable (replace 'h' with 'l')

        let data_str = String::from_utf8_lossy(data);

        // Check for mouse enable sequences (ending with 'h')
        if data_str.contains("\x1b[?1000h") ||
           data_str.contains("\x1b[?1002h") ||
           data_str.contains("\x1b[?1003h") ||
           data_str.contains("\x1b[?1006h") {
            self.mouse_mode_enabled = true;
        }

        // Check for mouse disable sequences (ending with 'l')
        if data_str.contains("\x1b[?1000l") ||
           data_str.contains("\x1b[?1002l") ||
           data_str.contains("\x1b[?1003l") ||
           data_str.contains("\x1b[?1006l") {
            self.mouse_mode_enabled = false;
        }
    }

    /// Sends a keyboard event to the shell.
    ///
    /// # Arguments
    /// * `key_event` - The key event to send
    pub fn send_key(&mut self, key_event: KeyEvent) -> Result<()> {
        let bytes = key_to_bytes(key_event);
        self.writer.write_all(&bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Sends raw bytes to the shell without any processing.
    ///
    /// # Arguments
    /// * `bytes` - The raw bytes to send
    pub fn send_raw_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Sends a mouse event to the shell.
    ///
    /// # Arguments
    /// * `mouse_event` - The mouse event to send
    pub fn send_mouse(&mut self, mouse_event: crossterm::event::MouseEvent) -> Result<()> {
        let bytes = mouse_to_bytes(mouse_event);
        self.writer.write_all(&bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Resizes the PTY to the specified dimensions.
    ///
    /// # Arguments
    /// * `cols` - New terminal width in columns
    /// * `rows` - New terminal height in rows
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        // Resize the PTY
        self.pty_master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        Ok(())
    }

}

/// Converts a crossterm key event to terminal byte sequence.
///
/// # Arguments
/// * `key_event` - The key event to convert
///
/// # Returns
/// The byte sequence representing the key event in terminal format
fn key_to_bytes(key_event: KeyEvent) -> Vec<u8> {
    let KeyEvent { code, modifiers, .. } = key_event;

    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);

    // eprintln!("DEBUG key_to_bytes: code={:?}, ctrl={}, alt={}", code, ctrl, alt);
    // let _ = stderr().flush();

    match code {
        KeyCode::Char(c) => {
            // eprintln!("DEBUG: Char '{}' (0x{:02x})", c, c as u8);
            // let _ = stderr().flush();
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

/// Converts a crossterm mouse event to xterm SGR mouse protocol bytes.
///
/// # Arguments
/// * `mouse_event` - The mouse event to convert
///
/// # Returns
/// The byte sequence representing the mouse event in xterm SGR format (1006 mode)
fn mouse_to_bytes(mouse_event: crossterm::event::MouseEvent) -> Vec<u8> {
    use crossterm::event::{MouseButton, MouseEventKind, KeyModifiers};

    // Convert mouse event to xterm SGR mouse protocol (1006 mode)
    // Format: ESC [ < Cb ; Cx ; Cy M (press) or m (release)
    // Reference: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Extended-coordinates
    //
    // Button codes:
    //   0=left, 1=middle, 2=right
    //   32=motion (drag)
    //   64=wheel up, 65=wheel down
    // Modifiers add to button code:
    //   +4 for Shift, +8 for Alt, +16 for Ctrl

    let (mut button_code, is_release) = match mouse_event.kind {
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
    if mouse_event.modifiers.contains(KeyModifiers::SHIFT) {
        button_code += 4;
    }
    if mouse_event.modifiers.contains(KeyModifiers::ALT) {
        button_code += 8;
    }
    if mouse_event.modifiers.contains(KeyModifiers::CONTROL) {
        button_code += 16;
    }

    // 1-based coordinates (terminal coordinates start at 1)
    let x = mouse_event.column + 1;
    let y = mouse_event.row + 1;

    // SGR mouse protocol
    let terminator = if is_release { b'm' } else { b'M' };
    format!("\x1b[<{};{};{}{}", button_code, x, y, terminator as char).into_bytes()
}
