//! Shell execution and process management module.
//!
//! This module handles shell subprocess creation, command execution,
//! and output capturing for the terminal interface.

mod subprocess;
pub use subprocess::ShellManager;
