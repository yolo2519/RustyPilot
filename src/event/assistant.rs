//! Key event handling for the AI Assistant pane.

use anyhow::Result;
use arboard::Clipboard;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::UserEvent;
use crate::ai::session::AiSessionManager;
use crate::ui::assistant::TuiAssistant;

/// Handle key events when the Assistant pane is active.
///
/// This function processes keyboard input for the assistant sidebar,
/// including text input, command confirmation, scrolling, and session switching.
///
/// # Arguments
/// * `assistant` - The assistant UI widget
/// * `ai_sessions` - The AI session manager (authoritative source for command suggestions)
/// * `key_evt` - The key event to handle
/// * `context_manager` - Current shell context for AI requests
pub fn handle_key_event(
    assistant: &mut TuiAssistant,
    ai_sessions: &mut AiSessionManager,
    context_manager: &crate::context::ContextManager,
    key_evt: KeyEvent,
) -> Result<()> {
    let session_id = assistant.active_session_id();

    // Check for pending command confirmation first (Ctrl+Y / Ctrl+N / Ctrl+/ shortcuts)
    // Use ai_sessions as the authoritative source for pending suggestions
    if ai_sessions.has_pending_suggestion(session_id) {
        match key_evt.code {
            // Ctrl+A => Cycle to next command suggestion
            KeyCode::Char('a') | KeyCode::Char('A')
                if key_evt.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                assistant.cycle_suggestion();
                return Ok(());
            }

            // Ctrl+Y => Execute or Copy the currently displayed command
            KeyCode::Char('y') | KeyCode::Char('Y')
            if key_evt.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Get the index of the currently displayed suggestion
                let pending_idx = assistant.current_suggestion_index();

                // Check if the command is denied (should copy instead of execute)
                if assistant.is_pending_command_denied() {
                    // Deny verdict: copy to clipboard instead of executing
                    if assistant.copy_pending_command().is_some() {
                        // Update backend state
                        ai_sessions.reject_suggestion(session_id);
                    }
                } else {
                    // Allow/RequireConfirmation verdict: execute the command
                    if let Some(command) = ai_sessions.accept_suggestion(session_id, pending_idx) {
                        // Update UI to show command as executed
                        assistant.confirm_command();

                        // Tell the session manager to execute the suggested command
                        // It will send the ExecuteAiCommand event to the app layer
                        // Security gating happens in app.rs try_execute_suggested()
                        ai_sessions.execute_suggestion(session_id, command)?;
                    }
                }

                return Ok(());
            }

            // Ctrl+N => Reject all command suggestions
            KeyCode::Char('n') | KeyCode::Char('N')
                if key_evt.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Update backend state first (marks all suggestions as Rejected)
                ai_sessions.reject_suggestion(session_id);
                // Update UI
                assistant.reject_command();
                return Ok(());
            }
            _ => {}
        }
    }
    let ctrl = key_evt.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key_evt.modifiers.contains(KeyModifiers::SHIFT);

    // Normal input handling
    match key_evt.code {
        // Ctrl+O: Insert a newline character (more reliable than Enter+modifier combos)
        KeyCode::Char('o') | KeyCode::Char('O') if ctrl => {
            assistant.delete_input_selection();
            assistant.insert_char('\n');
        }

        // Ctrl+A: Select all text in input
        KeyCode::Char('a') | KeyCode::Char('A') if ctrl => {
            assistant.select_all_input();
        }

        // Ctrl+C: Copy selected text (or do nothing if no selection)
        KeyCode::Char('c') | KeyCode::Char('C') if ctrl => {
            assistant.copy_input_selection();
        }

        // Ctrl+X: Cut selected text
        KeyCode::Char('x') | KeyCode::Char('X') if ctrl => {
            assistant.cut_input_selection();
        }

        // Ctrl+V: Paste from clipboard (replaces selection if any)
        KeyCode::Char('v') | KeyCode::Char('V') if ctrl => {
            // Delete selection first if present
            assistant.delete_input_selection();

            // Paste from clipboard
            if let Ok(mut clipboard) = Clipboard::new() {
                if let Ok(text) = clipboard.get_text() {
                    for c in text.chars() {
                        assistant.insert_char(c);
                    }
                }
            }
        }

        // Plain Enter: Submit the message
        KeyCode::Enter => {
            // Don't allow sending new messages while AI is still streaming a response
            if assistant.is_streaming() {
                return Ok(());
            }

            let input = assistant.take_input();
            if !input.trim().is_empty() {
                // If there's a pending command, auto-reject it before sending new message
                if ai_sessions.has_pending_suggestion(session_id) {
                    ai_sessions.reject_suggestion(session_id);
                    assistant.reject_command();
                }

                assistant.push_user_message(input.clone());
                assistant.start_assistant_message();
                // Send to AI backend - response will come through ai_stream channel
                let context = context_manager.snapshot();
                ai_sessions.send_message(session_id, &input, context);
            }
        }

        // Text input (with selection replacement)
        KeyCode::Char(c) if !ctrl => {
            assistant.insert_char_with_selection(c);
        }

        // Editing (with selection support)
        KeyCode::Backspace => {
            assistant.delete_char_with_selection();
        }
        KeyCode::Delete => {
            assistant.delete_char_forward_with_selection();
        }

        // Cursor movement with optional selection (Shift+Arrow)
        KeyCode::Left if shift => {
            assistant.move_cursor_with_selection(-1, true);
        }
        KeyCode::Right if shift => {
            assistant.move_cursor_with_selection(1, true);
        }
        KeyCode::Left => {
            // Clear selection and move cursor
            assistant.clear_input_selection();
            assistant.move_cursor(-1);
        }
        KeyCode::Right => {
            // Clear selection and move cursor
            assistant.clear_input_selection();
            assistant.move_cursor(1);
        }

        KeyCode::Home if shift => {
            assistant.start_input_selection();
            assistant.move_cursor_to_start();
        }
        KeyCode::Home => {
            assistant.clear_input_selection();
            assistant.move_cursor_to_start();
        }

        // Scroll to bottom with Ctrl+End (must come before plain End)
        KeyCode::End if ctrl => {
            assistant.scroll_to_bottom();
        }

        KeyCode::End if shift => {
            assistant.start_input_selection();
            assistant.move_cursor_to_end();
        }
        KeyCode::End => {
            assistant.clear_input_selection();
            assistant.move_cursor_to_end();
        }

        // Escape: Clear selection or exit scroll mode
        KeyCode::Esc => {
            if assistant.has_input_selection() {
                assistant.clear_input_selection();
            } else if assistant.is_scrolled() {
                assistant.scroll_to_bottom();
            }
        }

        // Scrolling (with Shift modifier for scroll, Ctrl+Shift for selection+scroll)
        KeyCode::Up if shift && ctrl => {
            assistant.scroll(-1);
        }
        KeyCode::Down if shift && ctrl => {
            assistant.scroll(1);
        }
        KeyCode::PageUp if shift => {
            assistant.scroll(-10);
        }
        KeyCode::PageDown if shift => {
            assistant.scroll(10);
        }

        // Plain Up/Down arrows - cursor movement in multi-line input (with optional selection)
        KeyCode::Up => {
            assistant.clear_input_selection();
            let input_area_width = assistant.input_area_width();
            assistant.move_cursor_up(input_area_width);
        }
        KeyCode::Down => {
            assistant.clear_input_selection();
            let input_area_width = assistant.input_area_width();
            assistant.move_cursor_down(input_area_width);
        }

        // Session switching (Tab / Shift+Tab)
        // Sessions are managed by the backend, frontend just displays
        KeyCode::Tab => {
            let new_id = if shift {
                ai_sessions.prev_session_id()
            } else {
                ai_sessions.next_session_id()
            };

            if let Some(id) = new_id {
                // Switch backend session
                ai_sessions.switch_session(id);
                // Switch frontend session and load messages
                assistant.switch_session(id);
                let messages = ai_sessions.get_session_messages(id);
                assistant.load_messages(messages);
                // Sync tab list (in case it changed)
                assistant.sync_session_tabs(ai_sessions.get_session_tabs());
            }
        }

        _ => {}
    }
    Ok(())
}

/// Handle command mode keys specific to Assistant pane.
///
/// Returns true if the event was handled.
pub fn handle_command_mode(
    assistant: &mut TuiAssistant,
    ai_sessions: &mut AiSessionManager,
    event: UserEvent,
) -> Result<bool> {
    match event {
        // t => create new AI session
        UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('t') | KeyCode::Char('T')) => {
            if let Ok(new_id) = ai_sessions.new_session() {
                assistant.switch_session(new_id);
                assistant.load_messages(vec![]);
                assistant.sync_session_tabs(ai_sessions.get_session_tabs());
            }
            Ok(true)
        }

        // w => close current AI session
        UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('w') | KeyCode::Char('W')) => {
            let current_id = assistant.active_session_id();
            if let Some(new_id) = ai_sessions.close_session(current_id) {
                assistant.switch_session(new_id);
                let messages = ai_sessions.get_session_messages(new_id);
                assistant.load_messages(messages);
                assistant.sync_session_tabs(ai_sessions.get_session_tabs());
            }
            Ok(true)
        }

        // ] => next AI session
        UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char(']')) => {
            if let Some(new_id) = ai_sessions.next_session_id() {
                ai_sessions.switch_session(new_id);
                assistant.switch_session(new_id);
                let messages = ai_sessions.get_session_messages(new_id);
                assistant.load_messages(messages);
                assistant.sync_session_tabs(ai_sessions.get_session_tabs());
            }
            Ok(true)
        }

        // [ => previous AI session
        UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('[')) => {
            if let Some(new_id) = ai_sessions.prev_session_id() {
                ai_sessions.switch_session(new_id);
                assistant.switch_session(new_id);
                let messages = ai_sessions.get_session_messages(new_id);
                assistant.load_messages(messages);
                assistant.sync_session_tabs(ai_sessions.get_session_tabs());
            }
            Ok(true)
        }

        _ => Ok(false),
    }
}
