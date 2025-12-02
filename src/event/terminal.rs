//! Key event handling for the Terminal pane.

use anyhow::Result;
use crossterm::event::{KeyEvent, KeyCode, KeyModifiers};

use crate::ui::terminal::TuiTerminal;
use crate::shell::ShellManager;

/// Handle key events when the Terminal pane is active.
///
/// This function processes keyboard input for the terminal emulator,
/// forwarding keystrokes to the PTY.
pub fn handle_key_event(
    _terminal: &mut TuiTerminal,
    shell: &mut ShellManager,
    key_evt: KeyEvent,
) -> Result<()> {
    // Convert key event to bytes and forward to shell
    let bytes = key_to_bytes(key_evt);
    if !bytes.is_empty() {
        shell.handle_user_input(&bytes)?;
    }

    Ok(())
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
