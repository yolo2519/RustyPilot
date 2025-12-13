//! Command execution gating with security verdict enforcement.
//!
//! This module provides a single execution entrypoint that enforces
//! security verdicts before allowing any command to be executed.

use super::Verdict;

/// Result of attempting to execute a command through the security gate
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionDecision {
    /// Command should be executed immediately
    Execute,
    /// Command requires user confirmation before execution
    RequireConfirmation { reason: String },
    /// Command is denied and should not be executed
    Deny { reason: String },
}

/// Evaluates a command and returns the execution decision based on its verdict.
///
/// This is the single entrypoint for all command execution decisions,
/// ensuring that security verdicts are consistently enforced.
///
/// # Arguments
/// * `cmd` - The command string to evaluate
/// * `verdict` - The security verdict for this command
///
/// # Returns
/// An `ExecutionDecision` indicating whether to execute, require confirmation, or deny
///
/// # Examples
/// ```
/// use rusty_term::security::{Verdict, executor::{gate_command, ExecutionDecision}};
///
/// // Safe command - execute immediately
/// let decision = gate_command("ls -la", &Verdict::Allow);
/// assert_eq!(decision, ExecutionDecision::Execute);
///
/// // Requires confirmation
/// let decision = gate_command("rm file.txt", &Verdict::RequireConfirmation("Requires confirmation".to_string()));
/// assert!(matches!(decision, ExecutionDecision::RequireConfirmation { .. }));
///
/// // Dangerous command - deny
/// let decision = gate_command("ls | grep", &Verdict::Deny("Contains dangerous shell operators".to_string()));
/// assert!(matches!(decision, ExecutionDecision::Deny { .. }));
/// ```
pub fn gate_command(cmd: &str, verdict: &Verdict) -> ExecutionDecision {
    match verdict {
        Verdict::Allow => ExecutionDecision::Execute,
        Verdict::RequireConfirmation(reason) => ExecutionDecision::RequireConfirmation {
            reason: format!("Command '{}': {}", cmd, reason),
        },
        Verdict::Deny(reason) => ExecutionDecision::Deny {
            reason: format!("Command '{}': {}", cmd, reason),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_allow_verdict() {
        let decision = gate_command("pwd", &Verdict::Allow);
        assert_eq!(decision, ExecutionDecision::Execute);

        let decision = gate_command("ls -la", &Verdict::Allow);
        assert_eq!(decision, ExecutionDecision::Execute);
    }

    #[test]
    fn test_gate_require_confirmation_verdict() {
        let verdict = Verdict::RequireConfirmation("Requires confirmation".to_string());
        let decision = gate_command("rm file.txt", &verdict);
        match decision {
            ExecutionDecision::RequireConfirmation { reason } => {
                assert!(reason.contains("rm file.txt"));
            }
            _ => panic!("Expected RequireConfirmation decision"),
        }
    }

    #[test]
    fn test_gate_deny_verdict() {
        let verdict = Verdict::Deny("Contains dangerous shell operators".to_string());
        let decision = gate_command("ls | grep test", &verdict);
        match decision {
            ExecutionDecision::Deny { reason } => {
                assert!(reason.contains("ls | grep test"));
            }
            _ => panic!("Expected Deny decision"),
        }
    }

    #[test]
    fn test_gate_all_verdicts() {
        // Allow verdict
        let decision = gate_command("pwd", &Verdict::Allow);
        assert_eq!(decision, ExecutionDecision::Execute);

        // RequireConfirmation verdict
        let verdict = Verdict::RequireConfirmation("Requires confirmation".to_string());
        let decision = gate_command("cat file", &verdict);
        assert!(matches!(decision, ExecutionDecision::RequireConfirmation { .. }));

        // Deny verdict
        let verdict = Verdict::Deny("Dangerous".to_string());
        let decision = gate_command("rm -rf /", &verdict);
        assert!(matches!(decision, ExecutionDecision::Deny { .. }));
    }

    #[test]
    fn test_gate_empty_command() {
        let verdict = Verdict::Deny("Empty command".to_string());
        let decision = gate_command("", &verdict);
        assert!(matches!(decision, ExecutionDecision::Deny { .. }));
    }

    #[test]
    fn test_gate_complex_commands() {
        // Git commands
        let decision = gate_command("git status", &Verdict::Allow);
        assert_eq!(decision, ExecutionDecision::Execute);

        let verdict = Verdict::Deny("git push is destructive".to_string());
        let decision = gate_command("git push", &verdict);
        assert!(matches!(decision, ExecutionDecision::Deny { .. }));

        // Commands with arguments
        let decision = gate_command("echo 'hello world'", &Verdict::Allow);
        assert_eq!(decision, ExecutionDecision::Execute);
    }
}
