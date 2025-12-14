//! Key event handling for the Terminal pane.

use anyhow::Result;
use crossterm::event::{KeyEvent, KeyEventKind, KeyCode, KeyModifiers};

use super::UserEvent;

use crate::ui::terminal::TuiTerminal;
use crate::shell::ShellManager;

/// Handle key events when the Terminal pane is active.
///
/// This function processes keyboard input for the terminal emulator,
/// forwarding keystrokes to the PTY and tracking command input.
pub fn handle_key_event(
    terminal: &mut TuiTerminal,
    shell: &mut ShellManager,
    key_evt: KeyEvent,
    shell_input_buffer: &mut String,
) -> Result<()> {
    let KeyEvent { code, modifiers, .. } = key_evt;
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);

    // Handle scrolling with Shift + PageUp/PageDown/Up/Down
    if shift {
        match code {
            KeyCode::PageUp => {
                terminal.scroll_up(10);
                return Ok(());
            }
            KeyCode::PageDown => {
                terminal.scroll_down(10);
                return Ok(());
            }
            KeyCode::Up => {
                terminal.scroll_up(1);
                return Ok(());
            }
            KeyCode::Down => {
                terminal.scroll_down(1);
                return Ok(());
            }
            KeyCode::End => {
                terminal.scroll_to_bottom();
                return Ok(());
            }
            _ => {}
        }
    }

    // For any other input, if scrolled back, auto-scroll to bottom
    if terminal.is_scrolled() {
        // Only auto-scroll for actual input keys (not just modifiers)
        if !matches!(code, KeyCode::Null) {
            terminal.scroll_to_bottom();
        }
    }

    // Track shell input for command logging (basic tracking)
    // Only track printable characters and common editing keys
    if !ctrl && !alt {
        match code {
            KeyCode::Char(c) => {
                shell_input_buffer.push(c);
            }
            KeyCode::Backspace => {
                shell_input_buffer.pop();
            }
            KeyCode::Enter => {
                // Record command in log if non-empty
                let cmd = shell_input_buffer.trim();
                if !cmd.is_empty() {
                    shell.start_new_command(cmd.to_string());
                }
                // Clear buffer after Enter
                shell_input_buffer.clear();
            }
            _ => {}
        }
    } else if ctrl {
        // Ctrl+C, Ctrl+D, etc. might interrupt command
        // Clear buffer on Ctrl+C or Ctrl+U (common line kill)
        match code {
            KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Char('u') | KeyCode::Char('U') => {
                shell_input_buffer.clear();
            }
            _ => {}
        }
    }

    // Convert key event to bytes and forward to shell
    let bytes = key_to_bytes(key_evt);
    if !bytes.is_empty() {
        shell.handle_user_input(&bytes)?;
    }

    Ok(())
}

/// Handle command mode keys specific to Terminal pane.
///
/// Returns true if the event was handled.
pub fn handle_command_mode(
    terminal: &mut TuiTerminal,
    shell: &mut ShellManager,
    shell_input_buffer: &mut String,
    event: UserEvent,
) -> Result<bool> {
    match event {
        // Forward Ctrl+B to terminal
        UserEvent::Key(e) if
            matches!(e.kind, KeyEventKind::Press)
         && matches!(e.modifiers, KeyModifiers::CONTROL)
         && matches!(e.code, KeyCode::Char('b') | KeyCode::Char('B')) => {
            handle_key_event(terminal, shell, e, shell_input_buffer)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Converts a crossterm key event to terminal byte sequence.
fn key_to_bytes(key_event: KeyEvent) -> Vec<u8> {
    let KeyEvent { code, modifiers, .. } = key_event;

    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);

    match code {
        KeyCode::Char(c) => {
            if ctrl {
                // Handle Ctrl+letter: Ctrl+A = 0x01, Ctrl+B = 0x02, etc.
                if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
                    let byte = (c.to_ascii_lowercase() as u8) - b'a' + 1;
                    vec![byte]
                } else if c == '@' {
                    vec![0x00]  // Ctrl+@ = NUL
                } else if c == '[' {
                    vec![0x1b]  // Ctrl+[ = ESC
                } else if c == '\\' {
                    vec![0x1c]  // Ctrl+\ = FS
                } else if c == ']' {
                    vec![0x1d]  // Ctrl+] = GS
                } else if c == '^' {
                    vec![0x1e]  // Ctrl+^ = RS
                } else if c == '_' {
                    vec![0x1f]  // Ctrl+_ = US
                } else if c == '?' {
                    vec![0x7f]  // Ctrl+? = DEL
                } else {
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
