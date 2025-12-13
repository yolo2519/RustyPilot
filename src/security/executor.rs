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
/// let decision = gate_command("ls -la", Verdict::Allow);
/// assert_eq!(decision, ExecutionDecision::Execute);
///
/// // Requires confirmation
/// let decision = gate_command("rm file.txt", Verdict::RequireConfirmation);
/// assert!(matches!(decision, ExecutionDecision::RequireConfirmation { .. }));
///
/// // Dangerous command - deny
/// let decision = gate_command("ls | grep", Verdict::Deny);
/// assert!(matches!(decision, ExecutionDecision::Deny { .. }));
/// ```
pub fn gate_command(cmd: &str, verdict: Verdict) -> ExecutionDecision {
    match verdict {
        Verdict::Allow => ExecutionDecision::Execute,
        Verdict::RequireConfirmation => ExecutionDecision::RequireConfirmation {
            reason: format!("Command '{}' requires user confirmation before execution", cmd),
        },
        Verdict::Deny => ExecutionDecision::Deny {
            reason: format!(
                "Command '{}' contains dangerous shell operators and cannot be executed",
                cmd
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_allow_verdict() {
        let decision = gate_command("pwd", Verdict::Allow);
        assert_eq!(decision, ExecutionDecision::Execute);

        let decision = gate_command("ls -la", Verdict::Allow);
        assert_eq!(decision, ExecutionDecision::Execute);
    }

    #[test]
    fn test_gate_require_confirmation_verdict() {
        let decision = gate_command("rm file.txt", Verdict::RequireConfirmation);
        match decision {
            ExecutionDecision::RequireConfirmation { reason } => {
                assert!(reason.contains("rm file.txt"));
                assert!(reason.contains("requires user confirmation"));
            }
            _ => panic!("Expected RequireConfirmation decision"),
        }
    }

    #[test]
    fn test_gate_deny_verdict() {
        let decision = gate_command("ls | grep test", Verdict::Deny);
        match decision {
            ExecutionDecision::Deny { reason } => {
                assert!(reason.contains("ls | grep test"));
                assert!(reason.contains("dangerous shell operators"));
            }
            _ => panic!("Expected Deny decision"),
        }
    }

    #[test]
    fn test_gate_all_verdicts() {
        let test_cases = vec![
            ("pwd", Verdict::Allow, ExecutionDecision::Execute),
            (
                "cat file",
                Verdict::RequireConfirmation,
                ExecutionDecision::RequireConfirmation {
                    reason: "Command 'cat file' requires user confirmation before execution"
                        .to_string(),
                },
            ),
            (
                "rm -rf /",
                Verdict::Deny,
                ExecutionDecision::Deny {
                    reason: "Command 'rm -rf /' contains dangerous shell operators and cannot be executed".to_string(),
                },
            ),
        ];

        for (cmd, verdict, expected) in test_cases {
            let decision = gate_command(cmd, verdict);
            assert_eq!(decision, expected, "Failed for command: {}", cmd);
        }
    }

    #[test]
    fn test_gate_empty_command() {
        let decision = gate_command("", Verdict::Deny);
        assert!(matches!(decision, ExecutionDecision::Deny { .. }));
    }

    #[test]
    fn test_gate_complex_commands() {
        // Git commands
        let decision = gate_command("git status", Verdict::Allow);
        assert_eq!(decision, ExecutionDecision::Execute);

        let decision = gate_command("git push", Verdict::Deny);
        assert!(matches!(decision, ExecutionDecision::Deny { .. }));

        // Commands with arguments
        let decision = gate_command("echo 'hello world'", Verdict::Allow);
        assert_eq!(decision, ExecutionDecision::Execute);
    }
}

