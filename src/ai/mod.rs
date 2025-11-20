//! AI module for managing LLM interactions and command suggestions.
//! 
//! This module provides functionality for communicating with AI services,
//! managing chat sessions, parsing AI responses, and building prompts.

pub mod client;
pub mod parser;
pub mod prompt;
pub mod session;

pub use client::AiClient;
pub use session::AiSessionManager;
