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

use std::thread;

use tokio::sync::mpsc::{self, Receiver, Sender};
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
#[non_exhaustive]
pub enum AppEvent {
    // TODO: Add specific event types as needed
}

/// Initializes the application event system.
///
/// Creates a channel pair for application-wide event communication. The sender
/// can be cloned and distributed to various components that need to emit events,
/// while the receiver is typically handled by the main application loop.
///
/// # Returns
///
/// A tuple containing:
/// - `Sender<AppEvent>`: For sending application events (can be cloned)
/// - `Receiver<AppEvent>`: For receiving and processing application events
///
/// # Usage
///
/// The sender should be passed to components that need to emit events,
/// while the receiver is used in the main event loop to handle these events.
pub fn init_app_eventsource() -> (Sender<AppEvent>, Receiver<AppEvent>) {
    mpsc::channel(2048)
}
