//! Visual mode state management for text selection and copying.
//!
//! This module provides shared data structures for Visual mode functionality
//! in both Terminal and Assistant panes.

use arboard::Clipboard;
use ratatui::style::Color;
use tracing::error;

// ============================================================================
// Pane Status API (for rendering title bar and hints)
// ============================================================================

/// Status information that a pane provides for rendering.
/// This allows components to control their title and hints without
/// exposing internal state details to App.
#[derive(Debug, Clone, Default)]
pub struct PaneStatus {
    /// Status text to show in title bar (e.g., "VISUAL | Scrolled ↑5")
    pub title_status: Option<String>,
    /// Hint text for the bottom bar
    pub hint_text: Option<&'static str>,
    /// Override border color (None = use default based on active state)
    pub border_color: Option<Color>,
}

impl PaneStatus {
    pub fn normal() -> Self {
        Self::default()
    }

    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.title_status = Some(status.into());
        self
    }

    pub fn with_hint(mut self, hint: &'static str) -> Self {
        self.hint_text = Some(hint);
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.border_color = Some(color);
        self
    }
}

/// Result of handling a key event in a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyHandleResult {
    /// Key was consumed by the component
    Consumed,
    /// Key was not handled, App should process it
    NotConsumed,
    /// Request to enter command mode (Ctrl+B in visual mode)
    RequestCommandMode,
}

/// Selection mode in visual mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// No selection, just cursor movement (VISUAL)
    #[default]
    None,
    /// Line-based selection from anchor to cursor (VISUAL SELECT)
    /// Selection is clamped to each line's effective width (no trailing spaces)
    Line,
    /// Block/rectangle selection (VISUAL BLOCK)
    /// Selection is the rectangle formed by anchor and cursor, as-is
    Block,
}

impl SelectionMode {
    /// Toggle between Line and Block modes (for use when already in selection)
    pub fn toggle_line_block(self) -> Self {
        match self {
            SelectionMode::None => SelectionMode::Line, // First time entering selection
            SelectionMode::Line => SelectionMode::Block,
            SelectionMode::Block => SelectionMode::Line,
        }
    }

    /// Get display name for status bar
    pub fn display_name(&self) -> Option<&'static str> {
        match self {
            SelectionMode::None => None,
            SelectionMode::Line => Some("VISUAL SELECT"),
            SelectionMode::Block => Some("VISUAL BLOCK"),
        }
    }
}

/// Visual mode cursor and selection state.
///
/// This struct tracks:
/// - The current cursor position (row, col)
/// - The selection anchor point and mode
/// - Vim-style repeat count (e.g., "5j" moves down 5 lines)
#[derive(Debug, Clone, Default)]
pub struct VisualState {
    /// Current cursor position (row, col) in the content coordinate system.
    /// Row 0 is the first line of content (may be in scrollback history).
    pub cursor: (usize, usize),

    /// Selection anchor point. When set, text between anchor and cursor is selected.
    /// The meaning depends on selection_mode.
    pub anchor: Option<(usize, usize)>,

    /// Current selection mode (None, Line, Block)
    pub selection_mode: SelectionMode,

    /// Vim-style repeat count for commands (e.g., "5j" = move down 5 lines).
    repeat_count: Option<usize>,
}

impl VisualState {
    /// Create a new VisualState with cursor at the given position.
    pub fn new(row: usize, col: usize) -> Self {
        Self {
            cursor: (row, col),
            anchor: None,
            selection_mode: SelectionMode::None,
            repeat_count: None,
        }
    }

    /// Get and consume the repeat count (returns 1 if not set).
    pub fn take_repeat_count(&mut self) -> usize {
        self.repeat_count.take().unwrap_or(1)
    }

    /// Accumulate a digit to the repeat count.
    pub fn accumulate_repeat_digit(&mut self, digit: usize) {
        let current = self.repeat_count.unwrap_or(0);
        // Limit to reasonable max to prevent overflow
        let new_count = current.saturating_mul(10).saturating_add(digit).min(9999);
        self.repeat_count = Some(new_count);
    }

    /// Check if repeat count is being accumulated.
    pub fn has_repeat_count(&self) -> bool {
        self.repeat_count.is_some()
    }

    /// Get current repeat count value (for display purposes).
    pub fn get_repeat_count(&self) -> Option<usize> {
        self.repeat_count
    }

    /// Clear repeat count without consuming.
    pub fn clear_repeat_count(&mut self) {
        self.repeat_count = None;
    }

    /// Check if selection mode is active (not None).
    pub fn is_selecting(&self) -> bool {
        self.selection_mode != SelectionMode::None
    }

    /// Get current selection mode.
    pub fn get_selection_mode(&self) -> SelectionMode {
        self.selection_mode
    }

    /// Toggle selection mode:
    /// - None -> Line (enter selection, set anchor)
    /// - Line <-> Block (toggle between selection types, keep anchor)
    pub fn cycle_selection_mode(&mut self) {
        let was_selecting = self.is_selecting();
        self.selection_mode = self.selection_mode.toggle_line_block();

        // Set anchor only when first entering selection mode
        if !was_selecting && self.anchor.is_none() {
            // Entering selection mode, set anchor at current position
            self.anchor = Some(self.cursor);
        }
        // If already has anchor (cycling Line -> Block), keep it
    }

    /// Get the selection range as (start, end) where start <= end.
    /// Returns None if not in selection mode.
    /// For Line mode: start and end define the text flow range.
    /// For Block mode: start is top-left, end is bottom-right of rectangle.
    pub fn selection_range(&self) -> Option<((usize, usize), (usize, usize))> {
        if self.selection_mode == SelectionMode::None {
            return None;
        }

        self.anchor.map(|anchor| {
            let (ar, ac) = anchor;
            let (cr, cc) = self.cursor;

            match self.selection_mode {
                SelectionMode::None => unreachable!(),
                SelectionMode::Line => {
                    // Line mode: order by row first, then column
                    if ar < cr || (ar == cr && ac <= cc) {
                        ((ar, ac), (cr, cc))
                    } else {
                        ((cr, cc), (ar, ac))
                    }
                }
                SelectionMode::Block => {
                    // Block mode: top-left to bottom-right rectangle
                    let min_row = ar.min(cr);
                    let max_row = ar.max(cr);
                    let min_col = ac.min(cc);
                    let max_col = ac.max(cc);
                    ((min_row, min_col), (max_row, max_col))
                }
            }
        })
    }

    /// Move cursor by delta, clamping to valid range.
    /// Returns true if the cursor actually moved.
    pub fn move_cursor(&mut self, delta_row: i32, delta_col: i32, max_row: usize, max_col: usize) -> bool {
        let old_cursor = self.cursor;

        let new_row = (self.cursor.0 as i32 + delta_row).clamp(0, max_row as i32) as usize;
        let new_col = (self.cursor.1 as i32 + delta_col).clamp(0, max_col as i32) as usize;

        self.cursor = (new_row, new_col);
        self.cursor != old_cursor
    }

    /// Set cursor to a specific position.
    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor = (row, col);
    }

    /// Clear selection (reset to None mode).
    pub fn clear_selection(&mut self) {
        self.anchor = None;
        self.selection_mode = SelectionMode::None;
    }
}

/// Copy text to system clipboard.
/// Returns true if successful.
pub fn copy_to_clipboard(text: &str) -> bool {
    match Clipboard::new() {
        Ok(mut clipboard) => {
            match clipboard.set_text(text) {
                Ok(()) => true,
                Err(e) => {
                    error!("Failed to copy to clipboard: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            error!("Failed to access clipboard: {}", e);
            false
        }
    }
}

/// Check if a position is within a LINE selection range.
/// This is for text-flow selection where each line from start to end is included.
pub fn is_in_line_selection(row: usize, col: usize, start: (usize, usize), end: (usize, usize)) -> bool {
    let (sr, sc) = start;
    let (er, ec) = end;

    if row < sr || row > er {
        return false;
    }

    if row == sr && row == er {
        // Single line selection
        col >= sc && col <= ec
    } else if row == sr {
        // First line of multi-line selection
        col >= sc
    } else if row == er {
        // Last line of multi-line selection
        col <= ec
    } else {
        // Middle line of multi-line selection
        true
    }
}

/// Check if a position is within a BLOCK (rectangle) selection range.
/// This is for rectangle selection where all cells in the rectangle are included.
pub fn is_in_block_selection(row: usize, col: usize, start: (usize, usize), end: (usize, usize)) -> bool {
    let (sr, sc) = start;
    let (er, ec) = end;

    row >= sr && row <= er && col >= sc && col <= ec
}

/// Check if a position is within a selection range, given the selection mode.
/// For Line mode, optionally clamps to effective line width.
/// `line_width_fn` returns the effective width (last non-space char + 1) for a given row.
pub fn is_in_selection_with_mode<F>(
    row: usize,
    col: usize,
    start: (usize, usize),
    end: (usize, usize),
    mode: SelectionMode,
    line_width_fn: F,
) -> bool
where
    F: Fn(usize) -> usize,
{
    match mode {
        SelectionMode::None => false,
        SelectionMode::Line => {
            // Clamp column to effective line width
            let line_width = line_width_fn(row);
            if col >= line_width {
                return false;
            }

            let (sr, sc) = start;
            let (er, ec) = end;

            if row < sr || row > er {
                return false;
            }

            if row == sr && row == er {
                // Single line: clamp both start and end
                let eff_sc = sc.min(line_width.saturating_sub(1));
                let eff_ec = ec.min(line_width.saturating_sub(1));
                col >= eff_sc && col <= eff_ec
            } else if row == sr {
                // First line: clamp start col
                let eff_sc = sc.min(line_width.saturating_sub(1));
                col >= eff_sc
            } else if row == er {
                // Last line: clamp end col
                let eff_ec = ec.min(line_width.saturating_sub(1));
                col <= eff_ec
            } else {
                // Middle line: all columns up to line width
                true
            }
        }
        SelectionMode::Block => {
            // Block mode: don't clamp, use raw rectangle
            is_in_block_selection(row, col, start, end)
        }
    }
}
