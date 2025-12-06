//! Event handling system for the application.
//!
//! This module provides a dual-channel event system that separates user input events
//! from application-wide events to ensure responsiveness. User events (keyboard, mouse)
//! are handled on a separate thread to prevent blocking, while app events provide
//! communication between different components of the application.
//!
//! # Architecture
//!
//! - **User Events**: Direct input from the terminal (keyboard, mouse, resize)
//! - **App Events**: Internal application events for component communication
//!
//! The separation allows the UI to remain responsive even when processing
//! intensive operations, as user input is always handled promptly.
//!
//! # Submodules
//!
//! - `assistant`: Key event handling for the AI Assistant pane
//! - `terminal`: Key event handling for the Terminal pane

pub mod assistant;
pub mod terminal;

use std::thread;

use tokio::sync::mpsc::{self, Receiver, UnboundedReceiver, UnboundedSender};
use std::io::Result;

/// Type alias for user input events from the terminal.
///
/// This represents all possible user interactions including keyboard input,
/// mouse events, and terminal resize events.
pub type UserEvent = crossterm::event::Event;

/// Initializes the user event stream.
///
/// Creates a channel for receiving user input events and spawns a dedicated
/// thread to read events from the terminal. This ensures that user input
/// is always processed promptly without being blocked by other operations.
///
/// # Returns
///
/// A receiver that yields `Result<UserEvent>` items. The `Result` wrapper
/// handles potential I/O errors when reading from the terminal.
///
/// # Implementation Details
///
/// The spawned thread continuously reads events using `crossterm::event::read()`
/// and sends them through the channel. If the receiver is dropped, the thread
/// will automatically terminate when the send operation fails.
pub fn init_user_event() -> Receiver<Result<UserEvent>> {
    let (tx, rx) = mpsc::channel(64);
    thread::spawn(move || {
        loop {
            if tx.blocking_send(crossterm::event::read()).is_err() {
                break;
            }
        }
    });
    rx
}

use crate::ai::session::SessionId;

// =============================================================================
// AI Stream Data (Dedicated Channel)
// =============================================================================

/// Data transmitted through the dedicated AI streaming channel.
///
/// This is separate from AppEvent because:
/// 1. Streaming chunks are high-frequency and should not flood the global event queue
/// 2. TuiAssistant is the only consumer of stream data
/// 3. End-of-stream must be in the same channel to preserve ordering with chunks
#[derive(Debug, Clone)]
pub enum AiStreamData {
    /// A chunk of text from the streaming response
    Chunk {
        session_id: SessionId,
        text: String,
    },
    /// The streaming response has completed
    End {
        session_id: SessionId,
    },
    /// An error occurred during streaming
    Error {
        session_id: SessionId,
        error: String,
    },
}

// =============================================================================
// Application Events (Global Event Channel)
// =============================================================================

/// Application-wide events for inter-component communication.
///
/// This enum defines events that can be sent between different parts of the
/// application, such as notifications from background tasks, state changes,
/// or requests for actions.
///
/// # Note
///
/// This enum is marked as `#[non_exhaustive]` to allow adding new event types
/// in the future without breaking existing code.
///
/// # Design Note
///
/// High-frequency streaming data (AI chunks, shell output) uses dedicated channels
/// instead of AppEvent to avoid flooding this queue. AppEvent is reserved for
/// low-frequency coordination events.
#[non_exhaustive]
pub enum AppEvent {
    // =========================================================================
    // AI Events (Low-frequency, requires App-level handling)
    // =========================================================================

    /// AI has suggested a command that needs user confirmation.
    AiCommandSuggestion {
        session_id: SessionId,
        command: String,
        explanation: String,
    },

    /// User has confirmed execution of the AI-suggested command.
    ExecuteAiCommand {
        session_id: SessionId,
    },

    // =========================================================================
    // Shell Events
    // =========================================================================

    /// Shell error occurred (e.g., PTY read error, shell exited)
    ShellError {
        message: String,
    },
    
    /// Shell command execution completed
    ShellCommandCompleted {
        command: String,
        exit_code: i32,
    },

    /// Shell produced output chunk (throttled to short snippets)
    ShellOutput {
        data: String,
    },
}

/// Initializes the application event system.
///
/// Creates an unbounded channel pair for application-wide event communication.
/// Unbounded is appropriate here because:
/// 1. AppEvent is low-frequency (command suggestions, completions, etc.)
/// 2. We need to send from within async contexts without blocking
/// 3. The event types are lightweight and won't cause memory issues
///
/// # Returns
///
/// A tuple containing:
/// - `UnboundedSender<AppEvent>`: For sending application events (can be cloned)
/// - `UnboundedReceiver<AppEvent>`: For receiving and processing application events
///
/// # Usage
///
/// The sender should be passed to components that need to emit events,
/// while the receiver is used in the main event loop to handle these events.
/// 
/// Unbounded is appropriate here because AppEvent is low-frequency and lightweight.
pub fn init_app_eventsource() -> (UnboundedSender<AppEvent>, UnboundedReceiver<AppEvent>) {
    mpsc::unbounded_channel()
}
