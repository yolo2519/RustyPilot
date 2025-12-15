//! RustyTerm - A terminal emulator with AI-powered command assistance
//!
//! This library provides the core functionality for RustyTerm, including:
//! - AI session management for command suggestions
//! - Context management (working directory, environment, history)
//! - Event handling for user input and application events
//! - Shell management for PTY interaction
//!
//! # Example
//!
//! ```no_run
//! use rusty_term::ai::AiSessionManager;
//! use rusty_term::context::ContextSnapshot;
//! use rusty_term::event::{init_app_eventsource, AiUiUpdate};
//!
//! #[tokio::main]
//! async fn main() {
//!     let (app_event_tx, mut app_event_rx) = init_app_eventsource();
//!
//!     // AiSessionManager now owns its own stream channel internally
//!     let mut manager = AiSessionManager::new(
//!         app_event_tx,
//!         "gpt-4o-mini"
//!     ).unwrap();
//!
//!     let context = ContextSnapshot {
//!         cwd: "/home/user".to_string(),
//!         env_vars: vec![],
//!         recent_history: vec![],
//!         recent_output: vec![],
//!         recent_commands: vec![],
//!     };
//!
//!     let session_id = manager.current_session_id();
//!     manager.send_message(session_id, "list all files", context);
//!
//!     // Use recv_ai_stream() to receive and process AI responses
//!     // The manager stores data internally and returns AiUiUpdate for display
//! }
//! ```

pub mod ai;
pub mod app;
pub mod context;
pub mod event;
pub mod security;
pub mod shell;
pub mod ui;
pub mod utils;

// Re-export commonly used types
pub use ai::AiSessionManager;
pub use app::{ActivePane, App};
pub use context::{ContextManager, ContextSnapshot};
pub use event::{init_app_eventsource, init_user_event, AiStreamData, AiUiUpdate, AppEvent, UserEvent};
