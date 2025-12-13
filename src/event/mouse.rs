//! Mouse event handling for the application.
//!
//! This module handles mouse events including:
//! - Click to switch active pane
//! - Drag to select text (visual mode)
//! - Double-click to select word
//! - Scroll wheel (passthrough or history scroll)
//! - Mouse passthrough to PTY when terminal program enables mouse mode
//! - Tab bar interactions (switch session, new session, close session)
//! - Command card button clicks (execute, cancel)
//! - Input box cursor positioning

use anyhow::Result;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::ai::session::AiSessionManager;
use crate::app::{ActivePane, MouseTarget};
use crate::shell::ShellManager;
use crate::ui::assistant::{MessageAreaClickResult, TabClickResult, TuiAssistant};
use crate::ui::layout::AppLayout;
use crate::ui::terminal::TuiTerminal;

/// Maximum time between clicks to count as double-click (in milliseconds)
pub const DOUBLE_CLICK_THRESHOLD_MS: u128 = 500;

/// Maximum time between clicks to count as triple-click (in milliseconds)
pub const TRIPLE_CLICK_THRESHOLD_MS: u128 = 800;

/// Maximum distance (in cells) between clicks to count as double-click
pub const DOUBLE_CLICK_DISTANCE: u16 = 2;

/// State for mouse drag operations (used for visual selection or separator resize)
#[derive(Debug, Clone, Copy)]
pub struct MouseDragState {
    /// Which pane the drag started in
    pub target: MouseTarget,
    /// Starting screen coordinates (used for detecting movement)
    pub start_col: u16,
    pub start_row: u16,
    /// Whether selection has actually started (drag moved beyond threshold)
    pub selection_started: bool,
    /// Whether this is an input box drag (for Assistant pane)
    pub is_input_box_drag: bool,
}

/// State for separator drag (pane resizing)
#[derive(Debug, Clone, Copy)]
pub struct SeparatorDragState {
    /// Starting column position of the drag
    pub start_col: u16,
    /// Initial split ratio when drag started
    pub initial_ratio: u16,
}

/// State for double/triple-click detection
#[derive(Debug, Clone)]
pub struct LastClickState {
    /// When the last click occurred
    pub time: std::time::Instant,
    /// Screen position of last click
    pub col: u16,
    pub row: u16,
    /// Which pane was clicked
    pub target: MouseTarget,
    /// Click count (1=single, 2=double, 3=triple)
    pub click_count: u8,
}

/// Click region within Assistant pane
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantRegion {
    TabBar,
    MessageArea,
    InputBox,
}

/// Determine which UI element is at the given screen position.
pub fn get_mouse_target(layout: &AppLayout, col: u16, row: u16) -> MouseTarget {
    let term_area = layout.terminal_area;
    let asst_area = layout.assistant_area;
    let sep_area = layout.separator_area;

    // Check separator first (it's thin and between the panes)
    if col >= sep_area.x && col < sep_area.x + sep_area.width
        && row >= sep_area.y && row < sep_area.y + sep_area.height
    {
        return MouseTarget::Separator;
    }

    // Check terminal pane
    if col >= term_area.x && col < term_area.x + term_area.width
        && row >= term_area.y && row < term_area.y + term_area.height
    {
        return MouseTarget::Terminal;
    }

    // Check assistant pane
    if col >= asst_area.x && col < asst_area.x + asst_area.width
        && row >= asst_area.y && row < asst_area.y + asst_area.height
    {
        return MouseTarget::Assistant;
    }

    MouseTarget::Outside
}

/// Determine which region within the Assistant pane is clicked.
pub fn get_assistant_region(
    mouse: &MouseEvent,
    layout: &AppLayout,
    assistant: &TuiAssistant,
) -> AssistantRegion {
    let inner = layout.assistant_inner;

    // Calculate relative row within the inner area
    let rel_row = mouse.row.saturating_sub(inner.y);

    // Tab bar is the first line
    if rel_row == 0 {
        return AssistantRegion::TabBar;
    }

    // Calculate input box height
    let input_box_height = assistant.calculate_input_box_height(inner.height, inner.width);

    // Input box is at the bottom
    let input_box_start = inner.height.saturating_sub(input_box_height);
    if rel_row >= input_box_start {
        return AssistantRegion::InputBox;
    }

    AssistantRegion::MessageArea
}

/// Check click count (1=single, 2=double, 3=triple) based on previous click state.
/// Returns the click count for this click.
pub fn get_click_count(
    last_click: &Option<LastClickState>,
    target: MouseTarget,
    col: u16,
    row: u16,
) -> u8 {
    if let Some(last) = last_click {
        // Check target matches
        if last.target != target {
            return 1;
        }

        // Check position is close enough
        let col_diff = (col as i32 - last.col as i32).unsigned_abs() as u16;
        let row_diff = (row as i32 - last.row as i32).unsigned_abs() as u16;
        if col_diff > DOUBLE_CLICK_DISTANCE || row_diff > DOUBLE_CLICK_DISTANCE {
            return 1;
        }

        // Check time threshold
        let elapsed = last.time.elapsed().as_millis();

        // For triple-click, use extended threshold and check if previous was double
        if last.click_count == 2 && elapsed <= TRIPLE_CLICK_THRESHOLD_MS {
            return 3;
        }

        // For double-click
        if last.click_count == 1 && elapsed <= DOUBLE_CLICK_THRESHOLD_MS {
            return 2;
        }
    }
    1
}

/// Check if this click constitutes a double-click (legacy helper).
pub fn check_double_click(
    last_click: &Option<LastClickState>,
    target: MouseTarget,
    col: u16,
    row: u16,
) -> bool {
    get_click_count(last_click, target, col, row) == 2
}

/// Result of mouse event handling that may require App-level action.
#[derive(Debug, Clone, Copy, Default)]
pub struct MouseEventResult {
    /// New split ratio to set (if separator was dragged)
    pub new_split_ratio: Option<u16>,
}

/// Handle mouse events from crossterm.
///
/// This is the main entry point for mouse event handling.
/// Returns a MouseEventResult that may contain actions for the App to perform.
pub fn handle_mouse_event(
    mouse: MouseEvent,
    layout: &AppLayout,
    terminal: &mut TuiTerminal,
    assistant: &mut TuiAssistant,
    shell: &mut ShellManager,
    ai_sessions: &mut AiSessionManager,
    active_pane: &mut ActivePane,
    drag_state: &mut Option<MouseDragState>,
    separator_drag: &mut Option<SeparatorDragState>,
    last_click: &mut Option<LastClickState>,
    current_split_ratio: u16,
) -> Result<MouseEventResult> {
    let target = get_mouse_target(layout, mouse.column, mouse.row);
    let mut result = MouseEventResult::default();

    match mouse.kind {
        MouseEventKind::Down(button) => {
            handle_mouse_down(
                target, mouse, button, layout, terminal, assistant, shell, ai_sessions,
                active_pane, drag_state, separator_drag, last_click, current_split_ratio,
            )?;
        }
        MouseEventKind::Up(button) => {
            handle_mouse_up(
                target, mouse, button, layout, terminal, assistant, shell,
                drag_state, separator_drag,
            )?;
        }
        MouseEventKind::Drag(button) => {
            result.new_split_ratio = handle_mouse_drag(
                target, mouse, button, layout, terminal, assistant, shell,
                drag_state, separator_drag,
            )?;
        }
        MouseEventKind::ScrollUp => {
            handle_scroll(target, mouse, -3, layout, terminal, assistant, shell)?;
        }
        MouseEventKind::ScrollDown => {
            handle_scroll(target, mouse, 3, layout, terminal, assistant, shell)?;
        }
        MouseEventKind::Moved => {
            // Ignore move without button (could be used for hover effects later)
        }
        _ => {}
    }

    Ok(result)
}

/// Handle mouse button down event.
fn handle_mouse_down(
    target: MouseTarget,
    mouse: MouseEvent,
    button: MouseButton,
    layout: &AppLayout,
    terminal: &mut TuiTerminal,
    assistant: &mut TuiAssistant,
    shell: &mut ShellManager,
    ai_sessions: &mut AiSessionManager,
    active_pane: &mut ActivePane,
    drag_state: &mut Option<MouseDragState>,
    separator_drag: &mut Option<SeparatorDragState>,
    last_click: &mut Option<LastClickState>,
    current_split_ratio: u16,
) -> Result<()> {
    // Handle middle-click paste first (works regardless of pane)
    if button == MouseButton::Middle {
        handle_middle_click_paste(target, terminal, assistant, shell, active_pane, layout)?;
        return Ok(());
    }

    // Handle separator drag start
    if target == MouseTarget::Separator && button == MouseButton::Left {
        *separator_drag = Some(SeparatorDragState {
            start_col: mouse.column,
            initial_ratio: current_split_ratio,
        });
        *drag_state = None;
        return Ok(());
    }

    // Check if this click switches pane - if so, only switch pane and do nothing else
    let is_pane_switch = match target {
        MouseTarget::Terminal => *active_pane != ActivePane::Terminal,
        MouseTarget::Assistant => *active_pane != ActivePane::Assistant,
        _ => false,
    };

    if is_pane_switch {
        // Switch pane and return - don't process further
        match target {
            MouseTarget::Terminal => *active_pane = ActivePane::Terminal,
            MouseTarget::Assistant => *active_pane = ActivePane::Assistant,
            _ => {}
        }
        // Clear any drag state and return
        *drag_state = None;
        return Ok(());
    }

    // Get click count for multi-click detection
    let click_count = if button == MouseButton::Left {
        get_click_count(last_click, target, mouse.column, mouse.row)
    } else {
        1
    };

    match target {
        MouseTarget::Terminal => {
            // Check if terminal program wants mouse events
            if terminal.is_mouse_mode_enabled() {
                // Passthrough to PTY
                shell.send_mouse(mouse, layout.terminal_inner)?;
            } else if button == MouseButton::Left {
                match click_count {
                    3 => {
                        // Triple-click: select entire line
                        select_terminal_line(mouse, layout, terminal)?;
                    }
                    2 => {
                        // Double-click: select word at position
                        select_terminal_word(mouse, layout, terminal)?;
                    }
                    _ => {
                        // Single click: prepare for potential drag selection
                        // Just record the starting position, don't start selection yet
                        start_terminal_drag(mouse, layout, terminal, drag_state)?;
                    }
                }
            }
        }
        MouseTarget::Assistant => {
            let region = get_assistant_region(&mouse, layout, assistant);

            if button == MouseButton::Left {
                match region {
                    AssistantRegion::MessageArea => {
                        // First check if this is a command card button click
                        let msg_area = layout.assistant_inner;
                        // Message area starts after tab bar (1 line)
                        let msg_area_y = msg_area.y + 1;
                        let click_result = assistant.get_message_click_result(
                            mouse.column,
                            mouse.row,
                            msg_area.x,
                            msg_area_y,
                        );

                        match click_result {
                            MessageAreaClickResult::NextCommand(_msg_idx) => {
                                // Cycle to next suggestion
                                assistant.cycle_suggestion();
                                return Ok(());
                            }
                            MessageAreaClickResult::ExecuteCommand(_msg_idx) => {
                                // Execute the pending command
                                let session_id = assistant.active_session_id();
                                let pending_idx = assistant.current_suggestion_index();
                                if let Some(command) = ai_sessions.accept_suggestion(session_id, pending_idx) {
                                    assistant.confirm_command();
                                    ai_sessions.execute_suggestion(session_id, command)?;
                                }
                                return Ok(());
                            }
                            MessageAreaClickResult::CancelCommand(_msg_idx) => {
                                // Cancel the pending command
                                let session_id = assistant.active_session_id();
                                ai_sessions.reject_suggestion(session_id);
                                assistant.reject_command();
                                return Ok(());
                            }
                            MessageAreaClickResult::None => {
                                // Normal message area click
                                match click_count {
                                    3 => {
                                        // Triple-click in message area: select entire line
                                        select_assistant_line(mouse, layout, assistant)?;
                                    }
                                    2 => {
                                        // Double-click in message area: select word
                                        select_assistant_word(mouse, layout, assistant)?;
                                    }
                                    _ => {
                                        // Single click: prepare for potential drag selection
                                        start_assistant_drag(mouse, layout, assistant, drag_state)?;
                                    }
                                }
                            }
                        }
                    }
                    AssistantRegion::TabBar => {
                        // Handle tab bar click
                        let inner = layout.assistant_inner;
                        let click_result = assistant.get_tab_click_result(mouse.column, inner.x);

                        match click_result {
                            TabClickResult::SwitchToTab(session_id) => {
                                // Switch to the clicked session
                                if ai_sessions.switch_session(session_id) {
                                    assistant.switch_session(session_id);
                                    let messages = ai_sessions.get_session_messages(session_id);
                                    assistant.load_messages(messages);
                                    assistant.sync_session_tabs(ai_sessions.get_session_tabs());
                                }
                            }
                            TabClickResult::NewTab => {
                                // Create a new session
                                if let Ok(new_id) = ai_sessions.new_session() {
                                    assistant.switch_session(new_id);
                                    assistant.load_messages(vec![]);
                                    assistant.sync_session_tabs(ai_sessions.get_session_tabs());
                                }
                            }
                            TabClickResult::CloseTab(session_id) => {
                                // Close the session
                                if let Some(new_id) = ai_sessions.close_session(session_id) {
                                    assistant.switch_session(new_id);
                                    let messages = ai_sessions.get_session_messages(new_id);
                                    assistant.load_messages(messages);
                                    assistant.sync_session_tabs(ai_sessions.get_session_tabs());
                                }
                            }
                            TabClickResult::None => {
                                // Clicked on empty area - do nothing
                            }
                        }
                    }
                    AssistantRegion::InputBox => {
                        // Exit visual mode if active
                        if assistant.is_visual_mode() {
                            assistant.exit_visual_mode();
                        }

                        // Click in input box - position cursor and start potential drag selection
                        let inner = layout.assistant_inner;
                        let input_box_height = assistant.calculate_input_box_height(inner.height, inner.width);
                        let input_box_y = inner.y + inner.height.saturating_sub(input_box_height);
                        // Input box has a top border, so inner starts 1 line down
                        let input_inner_y = input_box_y + 1;

                        // Calculate relative position within input box
                        let rel_col = mouse.column.saturating_sub(inner.x);
                        let rel_row = mouse.row.saturating_sub(input_inner_y);

                        // Check for Shift-click to extend selection
                        let extend_selection = mouse.modifiers.contains(crossterm::event::KeyModifiers::SHIFT);

                        // Handle double/triple click in input box
                        match click_count {
                            3 => {
                                // Triple-click: select all input
                                assistant.select_all_input();
                            }
                            2 => {
                                // Double-click: select word at cursor
                                assistant.clear_input_selection();
                                assistant.set_input_cursor_from_click(rel_col, rel_row);
                                assistant.select_input_word_at_cursor();
                            }
                            _ => {
                                // Single click: position cursor, optionally extend selection
                                assistant.set_input_cursor_from_click_with_selection(rel_col, rel_row, extend_selection);

                                // Set up drag state for potential selection drag
                                *drag_state = Some(MouseDragState {
                                    target: MouseTarget::Assistant,
                                    start_col: mouse.column,
                                    start_row: mouse.row,
                                    selection_started: false,
                                    is_input_box_drag: true,
                                });
                            }
                        }
                    }
                }
            }
        }
        MouseTarget::Separator => {
            // Separator left-click handled above
        }
        MouseTarget::Outside => {}
    }

    // Record this click for multi-click detection
    if button == MouseButton::Left {
        *last_click = Some(LastClickState {
            time: std::time::Instant::now(),
            col: mouse.column,
            row: mouse.row,
            target,
            click_count,
        });
    }

    Ok(())
}

/// Handle mouse button up event.
fn handle_mouse_up(
    target: MouseTarget,
    mouse: MouseEvent,
    _button: MouseButton,
    layout: &AppLayout,
    terminal: &mut TuiTerminal,
    assistant: &mut TuiAssistant,
    shell: &mut ShellManager,
    drag_state: &mut Option<MouseDragState>,
    separator_drag: &mut Option<SeparatorDragState>,
) -> Result<()> {
    // End separator drag if active
    if separator_drag.is_some() {
        *separator_drag = None;
        return Ok(());
    }

    // If terminal has mouse mode enabled, forward the event
    if target == MouseTarget::Terminal && terminal.is_mouse_mode_enabled() {
        shell.send_mouse(mouse, layout.terminal_inner)?;
    }

    // If we have an active drag, finalize the selection
    if let Some(state) = drag_state.take() {
        if state.selection_started {
            // Update final cursor position
            match state.target {
                MouseTarget::Terminal => {
                    update_terminal_selection(mouse, layout, terminal)?;
                }
                MouseTarget::Assistant => {
                    update_assistant_selection(mouse, layout, assistant)?;
                }
                _ => {}
            }
        } else {
            // Mouse was pressed but not dragged - exit visual mode if entered
            match state.target {
                MouseTarget::Terminal => {
                    if terminal.is_visual_mode() && !terminal.is_visual_selecting() {
                        terminal.exit_visual_mode();
                    }
                }
                MouseTarget::Assistant => {
                    if assistant.is_visual_mode() && !assistant.is_visual_selecting() {
                        assistant.exit_visual_mode();
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// Handle mouse drag event.
/// Returns Some(ratio) if separator was dragged and split ratio should be updated.
fn handle_mouse_drag(
    target: MouseTarget,
    mouse: MouseEvent,
    button: MouseButton,
    layout: &AppLayout,
    terminal: &mut TuiTerminal,
    assistant: &mut TuiAssistant,
    shell: &mut ShellManager,
    drag_state: &mut Option<MouseDragState>,
    separator_drag: &mut Option<SeparatorDragState>,
) -> Result<Option<u16>> {
    // Handle separator drag first
    if let Some(sep_state) = separator_drag.as_ref() {
        if button == MouseButton::Left {
            // Calculate new split ratio based on mouse position
            let full_width = layout.full_area.width;
            if full_width > 0 {
                // Calculate ratio from mouse column position
                // mouse.column is the current position, convert to percentage
                let new_ratio = ((mouse.column as u32 * 100) / full_width as u32) as u16;
                // Clamp to valid range (10-90%)
                let clamped_ratio = new_ratio.clamp(10, 90);

                // Only update if significantly different from initial to avoid jitter
                let delta = (clamped_ratio as i32 - sep_state.initial_ratio as i32).unsigned_abs();
                if delta >= 1 {
                    return Ok(Some(clamped_ratio));
                }
            }
        }
        return Ok(None);
    }

    // If terminal has mouse mode enabled and drag is in terminal, forward
    if target == MouseTarget::Terminal && terminal.is_mouse_mode_enabled() {
        shell.send_mouse(mouse, layout.terminal_inner)?;
        return Ok(None);
    }

    // Handle visual selection drag
    if let Some(state) = drag_state.as_mut() {
        if button != MouseButton::Left {
            return Ok(None);
        }

        match state.target {
            MouseTarget::Terminal => {
                // Check if we've moved enough to start selection
                if !state.selection_started {
                    let moved = has_moved_enough(mouse.column, mouse.row, state.start_col, state.start_row);
                    if moved {
                        state.selection_started = true;
                        // Now start the actual selection
                        terminal.start_visual_selection();
                    }
                }

                if state.selection_started {
                    update_terminal_selection(mouse, layout, terminal)?;
                }
            }
            MouseTarget::Assistant => {
                if state.is_input_box_drag {
                    // Input box drag selection
                    if !state.selection_started {
                        let moved = has_moved_enough(mouse.column, mouse.row, state.start_col, state.start_row);
                        if moved {
                            state.selection_started = true;
                            // Start input selection
                            assistant.start_input_selection();
                        }
                    }

                    if state.selection_started {
                        // Update input cursor position (selection extends from anchor)
                        let inner = layout.assistant_inner;
                        let input_box_height = assistant.calculate_input_box_height(inner.height, inner.width);
                        let input_box_y = inner.y + inner.height.saturating_sub(input_box_height);
                        let input_inner_y = input_box_y + 1;

                        let rel_col = mouse.column.saturating_sub(inner.x);
                        let rel_row = mouse.row.saturating_sub(input_inner_y);

                        assistant.set_input_cursor_from_click(rel_col, rel_row);
                    }
                } else {
                    // Message area drag selection (visual mode)
                    if !state.selection_started {
                        let moved = has_moved_enough(mouse.column, mouse.row, state.start_col, state.start_row);
                        if moved {
                            state.selection_started = true;
                            // Now start the actual selection
                            assistant.start_visual_selection();
                        }
                    }

                    if state.selection_started {
                        update_assistant_selection(mouse, layout, assistant)?;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(None)
}

/// Check if mouse has moved enough to be considered a drag (not just a click).
fn has_moved_enough(col: u16, row: u16, start_col: u16, start_row: u16) -> bool {
    let col_diff = (col as i32 - start_col as i32).unsigned_abs();
    let row_diff = (row as i32 - start_row as i32).unsigned_abs();
    col_diff > 0 || row_diff > 0
}

/// Handle scroll wheel events.
fn handle_scroll(
    target: MouseTarget,
    mouse: MouseEvent,
    delta: i32,
    layout: &AppLayout,
    terminal: &mut TuiTerminal,
    assistant: &mut TuiAssistant,
    shell: &mut ShellManager,
) -> Result<()> {
    match target {
        MouseTarget::Terminal => {
            // If terminal program has mouse mode enabled, forward scroll
            if terminal.is_mouse_mode_enabled() {
                shell.send_mouse(mouse, layout.terminal_inner)?;
            } else {
                // Scroll terminal history
                if delta < 0 {
                    terminal.scroll_up((-delta) as usize);
                } else {
                    terminal.scroll_down(delta as usize);
                }
            }
        }
        MouseTarget::Assistant => {
            // Scroll assistant message history (positive delta = scroll down)
            assistant.scroll(delta as i16);
        }
        _ => {}
    }

    Ok(())
}

/// Start a potential drag selection in terminal (mouse down, not yet dragging).
fn start_terminal_drag(
    mouse: MouseEvent,
    layout: &AppLayout,
    terminal: &mut TuiTerminal,
    drag_state: &mut Option<MouseDragState>,
) -> Result<()> {
    // If already in visual mode with selection, clear it first
    if terminal.is_visual_mode() {
        terminal.clear_visual_selection();
    } else {
        // Enter visual mode
        terminal.enter_visual_mode();
    }

    // Convert screen coordinates to terminal content coordinates
    let term_inner = layout.terminal_inner;
    let rel_col = mouse.column.saturating_sub(term_inner.x) as usize;
    let rel_row = mouse.row.saturating_sub(term_inner.y) as usize;

    // Set cursor position (this will be the anchor if drag starts)
    terminal.set_visual_cursor_from_screen(rel_row, rel_col);

    // Record drag state - selection not started yet
    *drag_state = Some(MouseDragState {
        target: MouseTarget::Terminal,
        start_col: mouse.column,
        start_row: mouse.row,
        selection_started: false,
        is_input_box_drag: false,
    });

    Ok(())
}

/// Start a potential drag selection in assistant (mouse down, not yet dragging).
fn start_assistant_drag(
    mouse: MouseEvent,
    layout: &AppLayout,
    assistant: &mut TuiAssistant,
    drag_state: &mut Option<MouseDragState>,
) -> Result<()> {
    // If already in visual mode with selection, clear it first
    if assistant.is_visual_mode() {
        assistant.clear_visual_selection();
    } else {
        // Enter visual mode
        assistant.enter_visual_mode();
    }

    // Convert screen coordinates to assistant content coordinates
    let asst_inner = layout.assistant_inner;
    let rel_col = mouse.column.saturating_sub(asst_inner.x) as usize;
    // Account for tab bar (1 line)
    let rel_row = mouse.row.saturating_sub(asst_inner.y).saturating_sub(1) as usize;

    // Set cursor position (this will be the anchor if drag starts)
    assistant.set_visual_cursor_from_screen(rel_row, rel_col);

    // Record drag state - selection not started yet
    *drag_state = Some(MouseDragState {
        target: MouseTarget::Assistant,
        start_col: mouse.column,
        start_row: mouse.row,
        selection_started: false,
        is_input_box_drag: false,
    });

    Ok(())
}

/// Select word at position in terminal (double-click).
fn select_terminal_word(
    mouse: MouseEvent,
    layout: &AppLayout,
    terminal: &mut TuiTerminal,
) -> Result<()> {
    // Enter visual mode if not already
    if !terminal.is_visual_mode() {
        terminal.enter_visual_mode();
    }

    // Convert screen coordinates to terminal content coordinates
    let term_inner = layout.terminal_inner;
    let rel_col = mouse.column.saturating_sub(term_inner.x) as usize;
    let rel_row = mouse.row.saturating_sub(term_inner.y) as usize;

    // Select word at this position
    terminal.select_word_at(rel_row, rel_col);

    Ok(())
}

/// Select word at position in assistant message area (double-click).
fn select_assistant_word(
    mouse: MouseEvent,
    layout: &AppLayout,
    assistant: &mut TuiAssistant,
) -> Result<()> {
    // Enter visual mode if not already
    if !assistant.is_visual_mode() {
        assistant.enter_visual_mode();
    }

    // Convert screen coordinates to assistant content coordinates
    let asst_inner = layout.assistant_inner;
    let rel_col = mouse.column.saturating_sub(asst_inner.x) as usize;
    // Account for tab bar (1 line)
    let rel_row = mouse.row.saturating_sub(asst_inner.y).saturating_sub(1) as usize;

    // Select word at this position
    assistant.select_word_at(rel_row, rel_col);

    Ok(())
}

/// Update visual selection in terminal pane during drag.
fn update_terminal_selection(
    mouse: MouseEvent,
    layout: &AppLayout,
    terminal: &mut TuiTerminal,
) -> Result<()> {
    if !terminal.is_visual_mode() {
        return Ok(());
    }

    // Convert screen coordinates to terminal content coordinates
    let term_inner = layout.terminal_inner;
    let rel_col = mouse.column.saturating_sub(term_inner.x) as usize;
    let rel_row = mouse.row.saturating_sub(term_inner.y) as usize;

    // Update cursor position (selection extends from anchor to cursor)
    terminal.set_visual_cursor_from_screen(rel_row, rel_col);

    Ok(())
}

/// Update visual selection in assistant pane during drag.
fn update_assistant_selection(
    mouse: MouseEvent,
    layout: &AppLayout,
    assistant: &mut TuiAssistant,
) -> Result<()> {
    if !assistant.is_visual_mode() {
        return Ok(());
    }

    // Convert screen coordinates to assistant content coordinates
    let asst_inner = layout.assistant_inner;
    let rel_col = mouse.column.saturating_sub(asst_inner.x) as usize;
    // Account for tab bar (1 line)
    let rel_row = mouse.row.saturating_sub(asst_inner.y).saturating_sub(1) as usize;

    // Update cursor position (selection extends from anchor to cursor)
    assistant.set_visual_cursor_from_screen(rel_row, rel_col);

    Ok(())
}

/// Handle middle-click paste (X11 style).
///
/// Pastes clipboard content to the active pane:
/// - Terminal: sends clipboard content to PTY
/// - Assistant InputBox: inserts at cursor position
fn handle_middle_click_paste(
    target: MouseTarget,
    _terminal: &mut TuiTerminal,
    assistant: &mut TuiAssistant,
    shell: &mut ShellManager,
    active_pane: &ActivePane,
    layout: &AppLayout,
) -> Result<()> {
    use arboard::Clipboard;

    // Try to get clipboard content
    let clipboard_text = match Clipboard::new() {
        Ok(mut clipboard) => match clipboard.get_text() {
            Ok(text) => text,
            Err(_) => return Ok(()), // No text in clipboard
        },
        Err(_) => return Ok(()), // Clipboard not available
    };

    if clipboard_text.is_empty() {
        return Ok(());
    }

    match target {
        MouseTarget::Terminal => {
            // Paste to terminal PTY
            shell.handle_user_input(clipboard_text.as_bytes())?;
        }
        MouseTarget::Assistant => {
            // Check if click is in input box
            let region = get_assistant_region(
                &MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Middle),
                    column: 0,
                    row: 0,
                    modifiers: crossterm::event::KeyModifiers::empty(),
                },
                layout,
                assistant,
            );

            if matches!(region, AssistantRegion::InputBox) || *active_pane == ActivePane::Assistant {
                // Insert clipboard text at cursor position in input box
                for c in clipboard_text.chars() {
                    assistant.insert_char(c);
                }
            }
        }
        _ => {}
    }

    Ok(())
}

/// Select entire line at position in terminal (triple-click).
fn select_terminal_line(
    mouse: MouseEvent,
    layout: &AppLayout,
    terminal: &mut TuiTerminal,
) -> Result<()> {
    // Enter visual mode if not already
    if !terminal.is_visual_mode() {
        terminal.enter_visual_mode();
    }

    // Convert screen coordinates to terminal content coordinates
    let term_inner = layout.terminal_inner;
    let rel_row = mouse.row.saturating_sub(term_inner.y) as usize;

    // Select entire line at this position
    terminal.select_line_at(rel_row);

    Ok(())
}

/// Select entire line at position in assistant message area (triple-click).
fn select_assistant_line(
    mouse: MouseEvent,
    layout: &AppLayout,
    assistant: &mut TuiAssistant,
) -> Result<()> {
    // Enter visual mode if not already
    if !assistant.is_visual_mode() {
        assistant.enter_visual_mode();
    }

    // Convert screen coordinates to assistant content coordinates
    let asst_inner = layout.assistant_inner;
    // Account for tab bar (1 line)
    let rel_row = mouse.row.saturating_sub(asst_inner.y).saturating_sub(1) as usize;

    // Select entire line at this position
    assistant.select_line_at(rel_row);

    Ok(())
}
