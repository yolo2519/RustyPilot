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
//! use rusty_term::event::{init_app_eventsource, AiStreamData};
//! use tokio::sync::mpsc;
//!
//! #[tokio::main]
//! async fn main() {
//!     let (ai_stream_tx, mut ai_stream_rx) = mpsc::channel(100);
//!     let (app_event_tx, mut app_event_rx) = init_app_eventsource();
//!     
//!     let mut manager = AiSessionManager::new(
//!         ai_stream_tx,
//!         app_event_tx,
//!         "gpt-4o-mini"
//!     );
//!     
//!     let context = ContextSnapshot {
//!         cwd: "/home/user".to_string(),
//!         env_vars: vec![],
//!         recent_history: vec![],
//!     };
//!     
//!     let session_id = manager.current_session_id();
//!     manager.send_message(session_id, "list all files", context);
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
pub use ai::{AiClient, AiSessionManager};
pub use app::{ActivePane, App};
pub use context::{ContextManager, ContextSnapshot};
pub use event::{init_app_eventsource, init_user_event, AiStreamData, AppEvent, UserEvent};

