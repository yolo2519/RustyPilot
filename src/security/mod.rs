//! Security module for command analysis and safety checks.
//!
//! This module provides functionality to analyze commands for safety,
//! maintain allowlists, and warn or block potentially dangerous operations.

mod allowlist;
mod analyzer;
pub mod executor;

pub use allowlist::{Allowlist, Verdict, evaluate};
pub use analyzer::analyze_command;
pub use executor::{ExecutionDecision, gate_command};

#[derive(Debug)]
pub enum CommandSafety {
    Safe,
    Warn(String),  // print warning message
    Block(String), // forbid execution
}
