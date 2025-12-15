//! AI module for managing LLM interactions and command suggestions.
//!
//! This module provides functionality for communicating with AI services,
//! managing chat sessions, parsing AI responses, and building prompts.

pub mod prompt;
pub mod session;

pub use session::AiSessionManager;
