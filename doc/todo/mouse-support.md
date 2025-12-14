# Mouse Support Implementation Plan

This document provides a comprehensive guide for implementing mouse support in rusty-term.
It is designed to be read by future AI assistants or developers to continue the implementation.

## Overview

rusty-term is a TUI terminal emulator using:
- `ratatui` for rendering
- `crossterm` for terminal input/output
- `alacritty_terminal` for terminal emulation

Mouse support enables:
1. Mouse passthrough to terminal programs (vim, tmux, etc.)
2. Visual text selection when terminal programs don't use mouse
3. UI interactions (tab switching, button clicks, pane resizing)

## Technical Foundation

### alacritty_terminal TermMode API

The `alacritty_terminal` crate provides `TermMode` bitflags for detecting terminal modes:

```rust
use alacritty_terminal::term::TermMode;

// Access via Term struct
let mode = term.mode();

// Mouse mode flags
TermMode::MOUSE_REPORT_CLICK  // ESC[?1000h - Basic click reporting
TermMode::MOUSE_MOTION        // ESC[?1002h - Button event tracking
TermMode::MOUSE_DRAG          // ESC[?1003h - Any event tracking
TermMode::MOUSE_MODE          // Combined: CLICK | MOTION | DRAG
TermMode::SGR_MOUSE           // ESC[?1006h - SGR extended mode (NOT included in MOUSE_MODE)

// IMPORTANT: Use intersects() not contains() for MOUSE_MODE
// Programs like tmux may only enable a subset of mouse modes
if mode.intersects(TermMode::MOUSE_MODE) {
    // Passthrough mouse events to PTY
}
```

**Note**: `MOUSE_MODE` is a bitmask combining multiple mouse reporting modes. Programs may
enable only a subset (e.g., tmux enables `MOUSE_REPORT_CLICK` but not `MOUSE_MOTION`).
Use `intersects()` to check if *any* mouse mode is enabled, not `contains()` which
requires *all* modes to be enabled.

### SGR Mouse Protocol

When forwarding mouse events to the shell, use SGR extended mouse protocol (1006 mode):

Format: `ESC [ < Cb ; Cx ; Cy M` (press) or `m` (release)

Button codes:
- 0=left, 1=middle, 2=right
- 32=motion (drag)
- 64=wheel up, 65=wheel down
- +4 for Shift, +8 for Alt, +16 for Ctrl

Coordinates are 1-based and relative to terminal inner area.

## Architecture

### Event Flow

```
crossterm::event::MouseEvent
        │
        ▼
   event::mouse::handle_mouse_event()
        │
        ├─► Terminal Pane
        │       │
        │       ├─► Mouse mode ON  → Passthrough to PTY (SGR format)
        │       └─► Mouse mode OFF → Visual select / scroll history
        │
        ├─► Assistant Pane
        │       │
        │       ├─► Tab bar → Switch/create/close session (Phase 2)
        │       ├─► Messages → Visual select / scroll / command card
        │       └─► Input → Cursor positioning (Phase 2)
        │
        └─► Separator → Drag to resize (Phase 3)
```

### Key Data Structures

Located in `src/event/mouse.rs`:

```rust
/// Target of mouse event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseTarget {
    Terminal,
    Assistant,
    Separator,
    Outside,
}

/// Click region within Assistant pane
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantRegion {
    TabBar,
    MessageArea,
    InputBox,
}

/// Mouse drag state for selection
#[derive(Debug, Clone, Copy)]
pub struct MouseDragState {
    /// Which pane the drag started in
    pub target: MouseTarget,
    /// Starting screen coordinates (used for detecting movement)
    pub start_col: u16,
    pub start_row: u16,
    /// Whether selection has actually started (drag moved beyond threshold)
    pub selection_started: bool,
}

/// State for double-click detection
#[derive(Debug, Clone)]
pub struct LastClickState {
    pub time: std::time::Instant,
    pub col: u16,
    pub row: u16,
    pub target: MouseTarget,
}
```

Located in `src/app.rs`:

```rust
/// Active UI pane (for mouse target enum)
pub enum MouseTarget {
    Terminal,
    Assistant,
    Separator,
    Outside,
}
```

### Module Structure

```
src/
├── main.rs              # Mouse capture enable/disable
├── app.rs               # MouseTarget enum, mouse_drag_state, last_click fields
├── event/
│   ├── mod.rs           # Event types
│   └── mouse.rs         # All mouse event handling logic (NEW)
├── ui/
│   ├── terminal.rs      # is_mouse_mode_enabled(), set_visual_cursor_from_screen()
│   └── assistant.rs     # set_visual_cursor_from_screen(), select_word_at()
└── shell/
    └── subprocess.rs    # send_mouse() for PTY passthrough
```

## Implementation Phases

### Phase 1: Core Functionality [COMPLETED]

All items completed:
- [x] Enable mouse capture in main.rs
- [x] TuiTerminal::is_mouse_mode_enabled() using TermMode API (uses `intersects`)
- [x] ShellManager::send_mouse() for SGR passthrough
- [x] Mouse event routing to event::mouse module
- [x] Pane detection (Terminal, Assistant, Separator)
- [x] Terminal mouse passthrough when program enables mouse mode
- [x] Terminal visual select (drag) when mouse mode disabled
- [x] Scroll wheel handling (passthrough or history scroll)
- [x] Click to switch active pane (without triggering other actions)
- [x] Double-click to select word (Terminal and Assistant message area)
- [x] Assistant visual select (drag) in message area
- [x] Assistant region detection (TabBar, MessageArea, InputBox)
- [x] Clear existing selection on new click

#### Files Modified

1. **src/main.rs** - Enable/disable mouse capture with crossterm
2. **src/ui/terminal.rs** - Mouse mode detection, visual cursor from screen coords
3. **src/ui/assistant.rs** - Visual cursor from screen coords, word selection
4. **src/shell/subprocess.rs** - send_mouse() method with SGR encoding
5. **src/app.rs** - MouseTarget enum, drag/click state fields
6. **src/event/mouse.rs** - All mouse event handling logic (NEW)
7. **src/event/mod.rs** - Declare mouse submodule

#### Key Implementation Details

##### 1. Mouse Mode Detection (TuiTerminal)

```rust
// src/ui/terminal.rs
use alacritty_terminal::term::TermMode;

impl TuiTerminal {
    /// Check if the terminal program has enabled mouse mode.
    /// Uses intersects() because programs may enable only a subset of modes.
    pub fn is_mouse_mode_enabled(&self) -> bool {
        self.term.mode().intersects(TermMode::MOUSE_MODE)
    }
}
```

##### 2. Pane Switch Behavior

When clicking to switch panes, only the pane switch occurs - no other mouse actions
(like selection) are triggered for that click:

```rust
// src/event/mouse.rs - handle_mouse_down()
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
    *drag_state = None;
    return Ok(());
}
```

##### 3. Drag Selection Flow

1. MouseDown: Enter visual mode, set cursor position, record drag state with `selection_started: false`
2. MouseDrag: Check if moved enough (`has_moved_enough()`), if yes set `selection_started: true` and call `start_visual_selection()` to set anchor
3. MouseDrag (continued): Update cursor position as drag continues
4. MouseUp: Finalize selection, or exit visual mode if no drag occurred

##### 4. Double-Click Detection

```rust
pub const DOUBLE_CLICK_THRESHOLD_MS: u128 = 500;
pub const DOUBLE_CLICK_DISTANCE: u16 = 2;

fn check_double_click(last_click: &Option<LastClickState>, target: MouseTarget, col: u16, row: u16) -> bool {
    // Check same target, time < 500ms, position within 2 cells
}
```

Double-click word selection is enabled for:
- Terminal pane (anywhere)
- Assistant pane (MessageArea only, NOT InputBox)

### Phase 2: Assistant Interactions [COMPLETED]

- [x] Tab click to switch session
- [x] "+" button click to create session
- [x] "x" button on active tab to close session
- [x] [Execute] [Cancel] buttons on command cards
- [x] Input box cursor positioning on click

#### Implementation Details

##### 1. Tab Bar Click Detection

New types in `src/ui/assistant.rs`:
```rust
/// Result of clicking on the tab bar
pub enum TabClickResult {
    SwitchToTab(SessionId),
    NewTab,
    CloseTab(SessionId),
    None,
}

/// Hit area for a tab
pub struct TabHitArea {
    pub start_x: u16,
    pub end_x: u16,
    pub session_id: Option<SessionId>,
    pub is_close_button: bool,
}
```

The `render_tab_bar()` function now:
- Tracks each tab's screen position in `cached_tab_positions`
- Renders a "×" close button after the active tab name
- The "+" button position is also tracked

##### 2. Command Card Button Detection

New types:
```rust
pub enum MessageAreaClickResult {
    ExecuteCommand(usize),
    CancelCommand(usize),
    None,
}

pub struct CommandCardHitArea {
    pub message_idx: usize,
    pub start_y: u16,
    pub end_y: u16,
    pub execute_btn: Option<(u16, u16)>,
    pub cancel_btn: Option<(u16, u16)>,
}
```

The `render_message_list()` function now tracks pending command card positions.
Button areas are only tracked for cards with `CommandStatus::Pending`.

##### 3. Input Box Cursor Positioning

New method `set_input_cursor_from_click(rel_col, rel_row)`:
- Converts screen coordinates to byte offset in input buffer
- Handles multi-line input with wrapping
- Accounts for prompt width on first line

##### 4. Mouse Event Handler Changes

`handle_mouse_event()` now takes `ai_sessions: &mut AiSessionManager` parameter.
`handle_mouse_down()` processes:
- Tab bar clicks: switch session, create new, close current
- Message area clicks: execute/cancel command card buttons
- Input box clicks: position cursor

### Phase 3: Advanced Features [COMPLETED]

- [x] Drag separator to resize panes
- [x] Middle-click paste (X11 style)
- [x] Triple-click to select line
- [x] Input box text selection (Shift+Arrow, mouse drag, copy/paste)

#### Implementation Details

##### 1. Separator Drag to Resize Panes

New state in `src/event/mouse.rs`:
```rust
pub struct SeparatorDragState {
    pub start_col: u16,
    pub initial_ratio: u16,
}
```

- MouseDown on separator starts drag, records initial split ratio
- MouseDrag calculates new ratio from mouse column position
- MouseUp ends drag, final ratio is applied
- Ratio clamped to 10-90% range

##### 2. Middle-Click Paste (X11 Style)

Handled in `handle_middle_click_paste()`:
- Terminal pane: Sends clipboard content directly to PTY
- Assistant InputBox: Inserts clipboard text at cursor position
- Uses `arboard` crate for clipboard access

##### 3. Triple-Click to Select Line

Extended click detection in `LastClickState` with `click_count` field:
- `get_click_count()` returns 1, 2, or 3 based on timing and position
- Triple-click threshold: 800ms (vs 500ms for double-click)
- New methods: `TuiTerminal::select_line_at()`, `TuiAssistant::select_line_at()`

##### 4. Input Box Text Selection

New state in `TuiAssistant`:
```rust
input_selection_anchor: Option<usize>,  // Byte offset of selection start
```

Features:
- **Mouse drag**: Drag in input box creates selection
- **Shift+Arrow**: Extends selection while moving cursor
- **Ctrl+A**: Select all input text
- **Ctrl+C**: Copy selection
- **Ctrl+X**: Cut selection
- **Ctrl+V**: Paste (replaces selection if any)
- **Double-click**: Select word at cursor
- **Triple-click**: Select all input

Rendering: `render_input_box()` highlights selected text with blue background

## Testing Checklist

### Phase 1 Tests [All Passed]

1. **Mouse passthrough**
   - Run `tmux` - verify mouse clicks work (switch panes, select text)
   - Run `vim` and enable mouse (`:set mouse=a`)
   - Verify clicks position cursor correctly
   - Verify scroll works in vim
   - Exit vim/tmux, verify mouse returns to visual select mode

2. **Visual select**
   - With shell prompt, drag to select text
   - Verify selection highlights correctly
   - Press `y` to copy, verify clipboard content
   - Click elsewhere to clear selection and start new one

3. **Scroll wheel**
   - When vim/tmux is running with mouse: scroll should scroll in program
   - When at shell prompt: scroll should scroll terminal history
   - In Assistant pane: scroll should scroll message history

4. **Pane switching**
   - Click Terminal pane when Assistant is active: should only activate Terminal
   - Click Assistant pane when Terminal is active: should only activate Assistant
   - Verify clicking to switch doesn't accidentally trigger selection

5. **Double-click**
   - Double-click on a word in Terminal: should select the word
   - Double-click on a word in Assistant message area: should select the word
   - Double-click in Assistant input box: should NOT enter visual mode

6. **Assistant selection**
   - Drag to select text in Assistant message area
   - Verify selection works correctly
   - Click elsewhere to clear and start new selection

## References

- [XTerm Control Sequences - Mouse Tracking](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Extended-coordinates)
- `examples/tui_shell/shell.rs` - Reference implementation
- `examples/tui_shell/main.rs` - Mouse event handling example
