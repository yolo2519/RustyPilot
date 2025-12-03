//! Key event handling for the AI Assistant pane.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ai::session::AiSessionManager;
use crate::context::ContextSnapshot;
use crate::ui::assistant::TuiAssistant;

/// Handle key events when the Assistant pane is active.
///
/// This function processes keyboard input for the assistant sidebar,
/// including text input, command confirmation, scrolling, and session switching.
///
/// # Arguments
/// * `assistant` - The assistant UI widget
/// * `ai_sessions` - The AI session manager
/// * `key_evt` - The key event to handle
/// * `context` - Current shell context for AI requests
pub fn handle_key_event(
    assistant: &mut TuiAssistant,
    ai_sessions: &mut AiSessionManager,
    key_evt: KeyEvent,
    context: ContextSnapshot,
) -> Result<()> {
    // Check for pending command confirmation first (Ctrl+Y / Ctrl+N shortcuts)
    if assistant.has_pending_command() {
        match key_evt.code {
            KeyCode::Char('y') | KeyCode::Char('Y')
                if key_evt.modifiers.contains(KeyModifiers::CONTROL) => {
                // Update UI to show command as executed
                assistant.confirm_command();

                // Tell the session manager to execute the suggested command
                // It will send the ExecuteAiCommand event to the app layer
                let session_id = assistant.active_session_id();
                ai_sessions.execute_suggestion(session_id)?;

                return Ok(());
            }
            KeyCode::Char('n') | KeyCode::Char('N')
                if key_evt.modifiers.contains(KeyModifiers::CONTROL) => {
                assistant.reject_command();
                return Ok(());
            }
            _ => {}
        }
    }

    // Normal input handling
    match key_evt.code {
        // Text input
        KeyCode::Char(c) => {
            assistant.insert_char(c);
        }

        // Editing
        KeyCode::Backspace => {
            assistant.delete_char();
        }
        KeyCode::Delete => {
            assistant.delete_char_forward();
        }

        // Cursor movement
        KeyCode::Left => {
            assistant.move_cursor(-1);
        }
        KeyCode::Right => {
            assistant.move_cursor(1);
        }
        KeyCode::Home => {
            assistant.move_cursor_to_start();
        }

        // Scroll to bottom with Shift+End or Ctrl+End (must come before plain End)
        KeyCode::End if key_evt.modifiers.contains(KeyModifiers::SHIFT)
                     || key_evt.modifiers.contains(KeyModifiers::CONTROL) => {
            assistant.scroll_to_bottom();
        }

        KeyCode::End => {
            assistant.move_cursor_to_end();
        }

        // Scrolling
        KeyCode::Up => {
            assistant.scroll(-1);
        }
        KeyCode::Down => {
            assistant.scroll(1);
        }
        KeyCode::PageUp => {
            assistant.scroll(-10);
        }
        KeyCode::PageDown => {
            assistant.scroll(10);
        }

        // Submit message (Enter) or insert newline (Shift+Enter)
        KeyCode::Enter => {
            if key_evt.modifiers.contains(KeyModifiers::SHIFT) {
                // Shift+Enter: Insert a newline character
                assistant.insert_char('\n');
            } else {
                // Enter: Submit the message
                let input = assistant.take_input();
                if !input.trim().is_empty() {
                    let session_id = assistant.active_session_id();
                    assistant.push_user_message(input.clone());
                    assistant.start_assistant_message();
                    // Send to AI backend - response will come through ai_stream channel
                    ai_sessions.send_message(session_id, &input);
                }
            }
        }

        // Session switching (Tab / Shift+Tab)
        KeyCode::Tab => {
            if key_evt.modifiers.contains(KeyModifiers::SHIFT) {
                assistant.prev_session();
            } else {
                assistant.next_session();
            }
        }

        _ => {}
    }
    Ok(())
}
