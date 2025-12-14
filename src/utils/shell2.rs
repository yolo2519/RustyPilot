//! Shell2: a separate, non-PTY subprocess used to gather system context for AI.
//!
//! This is intentionally isolated from the interactive PTY shell:
//! - read-only commands only (best-effort)
//! - time-bounded
//! - output-bounded

use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::context::ContextSnapshot;

#[derive(Clone, Debug)]
pub struct Shell2Config {
    /// Per-command timeout.
    pub per_cmd_timeout: Duration,
    /// Max bytes captured per command (stdout + stderr combined).
    pub max_output_bytes: usize,
    /// Max lines kept per command output.
    pub max_lines: usize,
}

impl Default for Shell2Config {
    fn default() -> Self {
        Self {
            per_cmd_timeout: Duration::from_millis(450),
            max_output_bytes: 8 * 1024,
            max_lines: 60,
        }
    }
}

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
}

fn truncate_lines(s: &str, max_lines: usize) -> String {
    let mut out = String::new();
    for (i, line) in s.lines().enumerate() {
        if i >= max_lines {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn truncate_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut out = s[..max_bytes].to_string();
    out.push_str("\n…(truncated)…\n");
    out
}

async fn run_shell_cmd(cfg: &Shell2Config, cwd: &str, cmd: &str) -> Option<String> {
    let shell = default_shell();
    let mut c = Command::new(shell);
    c.arg("-lc").arg(cmd);
    c.current_dir(cwd);
    c.kill_on_drop(true);

    let res = timeout(cfg.per_cmd_timeout, c.output()).await.ok()?;
    let res = res.ok()?;

    // Merge stdout+stderr (stderr often contains useful signals like "not a git repo")
    let mut merged = Vec::new();
    merged.extend_from_slice(&res.stdout);
    if !res.stderr.is_empty() {
        if !merged.is_empty() && *merged.last().unwrap_or(&b'\n') != b'\n' {
            merged.push(b'\n');
        }
        merged.extend_from_slice(&res.stderr);
    }

    if merged.is_empty() {
        return None;
    }

    let s = String::from_utf8_lossy(&merged);
    let s = truncate_bytes(&s, cfg.max_output_bytes);
    let s = truncate_lines(&s, cfg.max_lines);
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Collect best-effort system context from a separate shell process.
///
/// This is designed to be fast and safe:
/// - short timeouts
/// - bounded output
/// - read-only commands (best-effort)
pub async fn collect_shell2_system_context(ctx: &ContextSnapshot) -> String {
    let cfg = Shell2Config::default();
    let cwd = ctx.cwd.as_str();

    // Keep this minimal; expand later if needed.
    let checks: &[(&str, &str)] = &[
        ("uname", "uname -srm"),
        ("whoami", "whoami"),
        // Directory glimpse (bounded)
        ("ls", "ls -1 2>/dev/null | head -n 40"),
        // Git context (bounded, best-effort)
        (
            "git",
            "command -v git >/dev/null 2>&1 && \
             git rev-parse --is-inside-work-tree 2>/dev/null && \
             git status --porcelain=v1 -b 2>/dev/null | head -n 60 || true",
        ),
    ];

    let mut out = String::new();
    for (label, cmd) in checks {
        if let Some(text) = run_shell_cmd(&cfg, cwd, cmd).await {
            out.push_str(label);
            out.push_str(":\n");
            out.push_str(&text);
            out.push('\n');
        }
    }

    out.trim().to_string()
}


