//! Key event handling for the Terminal pane.

use anyhow::Result;
use crossterm::event::{KeyEvent, KeyCode};

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
    // Convert key event to text and forward to shell
    // TODO: this is fake for debugging.
    let input = match key_evt.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "\n".to_string(),
        KeyCode::Tab => "\t".to_string(),
        KeyCode::Backspace => "\x08".to_string(), // Backspace character
        _ => return Ok(()), // Ignore other keys for now
    };

    // Send input to shell manager (which will echo it to pty_output)
    shell.handle_user_input(&input)?;

    Ok(())
}
