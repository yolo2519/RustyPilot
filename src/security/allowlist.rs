//! Command allowlist management.
//!
//! This module maintains a list of allowed command prefixes that can be
//! executed without additional security warnings or blocks.

use std::collections::HashSet;

/// Verdict for command evaluation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    RequireConfirmation,
    Deny,
}

pub struct Allowlist {
    allowed_prefixes: HashSet<String>,
}

impl Default for Allowlist {
    fn default() -> Self {
        Self::new()
    }
}

impl Allowlist {
    pub fn new() -> Self {
        Self {
            allowed_prefixes: HashSet::new(),
        }
    }

    pub fn allow_prefix(&mut self, prefix: &str) {
        self.allowed_prefixes.insert(prefix.to_string());
    }

    pub fn is_allowed(&self, cmd: &str) -> bool {
        self.allowed_prefixes
            .iter()
            .any(|p| cmd.trim_start().starts_with(p))
    }
}

/// Evaluate a command and return a verdict on whether it can be executed.
///
/// # Rules
/// - Deny: Commands containing shell composition tokens (|, ;, &&, ||, >, <, $(, backticks, &)
/// - Allow: Minimal safe set (pwd, ls, whoami, date, uname, which, echo)
/// - Git policy: Allow status/diff/log/show, deny push/reset/clean/..., others require confirmation
/// - Default: RequireConfirmation for other commands
pub fn evaluate(cmd: &str) -> Verdict {
    let trimmed = cmd.trim();
    
    // Empty command
    if trimmed.is_empty() {
        return Verdict::Deny;
    }
    
    // Check for dangerous shell composition tokens
    if contains_shell_composition(trimmed) {
        return Verdict::Deny;
    }
    
    // Extract the base command (first word)
    let base_cmd = trimmed.split_whitespace().next().unwrap_or("");
    
    // Check minimal safe set
    if is_safe_command(base_cmd) {
        return Verdict::Allow;
    }
    
    // Handle git commands specially
    if base_cmd == "git" {
        return evaluate_git_command(trimmed);
    }
    
    // Default: require confirmation
    Verdict::RequireConfirmation
}

/// Check if command contains dangerous shell composition tokens
fn contains_shell_composition(cmd: &str) -> bool {
    // Check for pipe
    if cmd.contains('|') {
        return true;
    }
    
    // Check for semicolon
    if cmd.contains(';') {
        return true;
    }
    
    // Check for logical operators
    if cmd.contains("&&") || cmd.contains("||") {
        return true;
    }
    
    // Check for redirects
    if cmd.contains('>') || cmd.contains('<') {
        return true;
    }
    
    // Check for command substitution
    if cmd.contains("$(") || cmd.contains('`') {
        return true;
    }
    
    // Check for background execution
    // Note: we need to check for & but not within && (already checked above)
    // A simple approach: if & exists and && doesn't explain all &, it's dangerous
    let ampersand_count = cmd.matches('&').count();
    let double_ampersand_count = cmd.matches("&&").count();
    if ampersand_count > double_ampersand_count * 2 {
        return true;
    }
    
    false
}

/// Check if command is in the minimal safe set
fn is_safe_command(cmd: &str) -> bool {
    matches!(cmd, "pwd" | "ls" | "whoami" | "date" | "uname" | "which" | "echo")
}

/// Evaluate git subcommand policy
fn evaluate_git_command(cmd: &str) -> Verdict {
    // Extract git subcommand (second word after "git")
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    
    if parts.len() < 2 {
        // Just "git" without subcommand
        return Verdict::Allow;
    }
    
    let subcommand = parts[1];
    
    // Allow safe read-only git commands
    if matches!(subcommand, "status" | "diff" | "log" | "show") {
        return Verdict::Allow;
    }
    
    // Deny dangerous git commands
    if matches!(
        subcommand,
        "push" | "reset" | "clean" | "rebase" | "force" | "branch" | "checkout" | "merge" | "pull"
    ) {
        return Verdict::Deny;
    }
    
    // Other git commands require confirmation
    Verdict::RequireConfirmation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_command() {
        assert_eq!(evaluate(""), Verdict::Deny);
        assert_eq!(evaluate("   "), Verdict::Deny);
    }

    #[test]
    fn test_safe_commands() {
        assert_eq!(evaluate("pwd"), Verdict::Allow);
        assert_eq!(evaluate("ls"), Verdict::Allow);
        assert_eq!(evaluate("ls -la"), Verdict::Allow);
        assert_eq!(evaluate("whoami"), Verdict::Allow);
        assert_eq!(evaluate("date"), Verdict::Allow);
        assert_eq!(evaluate("uname"), Verdict::Allow);
        assert_eq!(evaluate("uname -a"), Verdict::Allow);
        assert_eq!(evaluate("which bash"), Verdict::Allow);
        assert_eq!(evaluate("echo hello"), Verdict::Allow);
        assert_eq!(evaluate("  echo hello  "), Verdict::Allow);
    }

    #[test]
    fn test_shell_composition_pipe() {
        assert_eq!(evaluate("ls | grep test"), Verdict::Deny);
        assert_eq!(evaluate("cat file | wc -l"), Verdict::Deny);
    }

    #[test]
    fn test_shell_composition_semicolon() {
        assert_eq!(evaluate("ls; pwd"), Verdict::Deny);
        assert_eq!(evaluate("echo a; echo b"), Verdict::Deny);
    }

    #[test]
    fn test_shell_composition_logical() {
        assert_eq!(evaluate("ls && pwd"), Verdict::Deny);
        assert_eq!(evaluate("ls || pwd"), Verdict::Deny);
        assert_eq!(evaluate("test -f file && cat file"), Verdict::Deny);
    }

    #[test]
    fn test_shell_composition_redirect() {
        assert_eq!(evaluate("echo test > file.txt"), Verdict::Deny);
        assert_eq!(evaluate("cat < input.txt"), Verdict::Deny);
        assert_eq!(evaluate("ls >> output.log"), Verdict::Deny);
    }

    #[test]
    fn test_shell_composition_subshell() {
        assert_eq!(evaluate("echo $(pwd)"), Verdict::Deny);
        assert_eq!(evaluate("rm -rf $(find .)"), Verdict::Deny);
        assert_eq!(evaluate("echo `date`"), Verdict::Deny);
    }

    #[test]
    fn test_shell_composition_background() {
        assert_eq!(evaluate("sleep 100 &"), Verdict::Deny);
        assert_eq!(evaluate("long_process &"), Verdict::Deny);
        // Note: "&&" should be caught by logical operator check, not background
    }

    #[test]
    fn test_git_safe_commands() {
        assert_eq!(evaluate("git status"), Verdict::Allow);
        assert_eq!(evaluate("git diff"), Verdict::Allow);
        assert_eq!(evaluate("git log"), Verdict::Allow);
        assert_eq!(evaluate("git log --oneline"), Verdict::Allow);
        assert_eq!(evaluate("git show HEAD"), Verdict::Allow);
        assert_eq!(evaluate("git"), Verdict::Allow);
    }

    #[test]
    fn test_git_dangerous_commands() {
        assert_eq!(evaluate("git push"), Verdict::Deny);
        assert_eq!(evaluate("git push origin main"), Verdict::Deny);
        assert_eq!(evaluate("git reset"), Verdict::Deny);
        assert_eq!(evaluate("git reset --hard"), Verdict::Deny);
        assert_eq!(evaluate("git clean"), Verdict::Deny);
        assert_eq!(evaluate("git clean -fd"), Verdict::Deny);
        assert_eq!(evaluate("git rebase"), Verdict::Deny);
        assert_eq!(evaluate("git branch -D feature"), Verdict::Deny);
        assert_eq!(evaluate("git checkout main"), Verdict::Deny);
        assert_eq!(evaluate("git merge"), Verdict::Deny);
        assert_eq!(evaluate("git pull"), Verdict::Deny);
    }

    #[test]
    fn test_git_other_commands() {
        assert_eq!(evaluate("git add ."), Verdict::RequireConfirmation);
        assert_eq!(evaluate("git commit -m 'test'"), Verdict::RequireConfirmation);
        assert_eq!(evaluate("git stash"), Verdict::RequireConfirmation);
        assert_eq!(evaluate("git tag v1.0"), Verdict::RequireConfirmation);
    }

    #[test]
    fn test_other_commands() {
        assert_eq!(evaluate("cat file.txt"), Verdict::RequireConfirmation);
        assert_eq!(evaluate("cd /tmp"), Verdict::RequireConfirmation);
        assert_eq!(evaluate("mkdir test"), Verdict::RequireConfirmation);
        assert_eq!(evaluate("rm file.txt"), Verdict::RequireConfirmation);
        assert_eq!(evaluate("cp a b"), Verdict::RequireConfirmation);
        assert_eq!(evaluate("mv a b"), Verdict::RequireConfirmation);
    }

    #[test]
    fn test_complex_safe_commands() {
        assert_eq!(evaluate("ls -lah /home"), Verdict::Allow);
        assert_eq!(evaluate("echo 'hello world'"), Verdict::Allow);
        assert_eq!(evaluate("which python3"), Verdict::Allow);
    }

    #[test]
    fn test_edge_cases() {
        // Command with extra spaces
        assert_eq!(evaluate("  ls  -la  "), Verdict::Allow);
        
        // Git with composition should still be denied
        assert_eq!(evaluate("git status | grep modified"), Verdict::Deny);
        
        // Safe command with redirect should be denied
        assert_eq!(evaluate("pwd > output.txt"), Verdict::Deny);
    }
}
