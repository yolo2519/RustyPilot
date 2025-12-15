//! Shell2: a separate, non-PTY subprocess used to gather system context for AI.
//!
//! This is intentionally isolated from the interactive PTY shell:
//! - read-only commands only (best-effort)
//! - time-bounded
//! - output-bounded

use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;
use std::process::Stdio;

#[derive(Clone, Debug)]
pub struct Shell2Config {
    /// Per-command timeout.
    pub total_timeout: Duration,
    /// Max bytes captured per command (stdout + stderr combined).
    pub max_output_bytes: usize,
    /// Max lines kept per command output.
    pub max_lines: usize,
}

impl Default for Shell2Config {
    fn default() -> Self {
        Self {
            total_timeout: Duration::from_millis(600),
            max_output_bytes: 8 * 1024,
            max_lines: 60,
        }
    }
}

fn default_shell() -> String {
    // Prefer the user's configured shell, but keep a safe fallback.
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
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

fn truncate_bytes_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    // Find a valid UTF-8 boundary <= max_bytes.
    let mut cut = 0usize;
    for (idx, _) in s.char_indices() {
        if idx > max_bytes {
            break;
        }
        cut = idx;
    }
    let mut out = s[..cut].to_string();
    out.push_str("\n...(truncated)...\n");
    out
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Shell2Intent {
    pub want_git: bool,
    pub want_fs: bool,
    pub want_tools: bool,
}

async fn run_shell_script(cfg: &Shell2Config, cwd: &str, script: &str) -> Option<String> {
    let shell = default_shell();
    let mut c = Command::new(shell);
    // Use non-login shell to avoid slow profile scripts and reduce variability.
    c.arg("-c").arg(script);
    c.current_dir(cwd);
    c.kill_on_drop(true);
    c.stdin(Stdio::null());

    let res = timeout(cfg.total_timeout, c.output()).await.ok()?;
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
    let s = truncate_bytes_utf8(&s, cfg.max_output_bytes);
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
pub async fn collect_shell2_system_context(cwd: &str) -> String {
    collect_shell2_system_context_with_intent(cwd, Shell2Intent::default()).await
}

/// Same as `collect_shell2_system_context`, but allows choosing which checks to run.
pub async fn collect_shell2_system_context_with_intent(cwd: &str, intent: Shell2Intent) -> String {
    let mut cfg = Shell2Config::default();
    // Tool version probes can be slightly slower than uname/whoami; allow a bit more time when requested.
    if intent.want_tools {
        cfg.total_timeout = Duration::from_millis(1000);
    }

    // Build a single script to minimize subprocess overhead.
    // Each block is best-effort and bounded (via head).
    let mut script = String::new();
    script.push_str("set -u; ");

    script.push_str("printf 'uname:\\n'; uname -srm 2>/dev/null || true; printf '\\n'; ");
    script.push_str("printf 'whoami:\\n'; whoami 2>/dev/null || true; printf '\\n'; ");

    if intent.want_tools {
        // Tool versions are often crucial for debugging build/tooling issues, but keep it bounded and best-effort.
        script.push_str("printf 'tools:\\n'; ");
        script.push_str("for t in rustc cargo git node npm python3 python pip; do ");
        script.push_str("command -v \"$t\" >/dev/null 2>&1 || continue; ");
        script.push_str("printf '%s: ' \"$t\"; ");
        script.push_str("case \"$t\" in ");
        script.push_str("python|python3) \"$t\" --version 2>&1 | head -n 1 || true ;; ");
        script.push_str("pip) \"$t\" --version 2>&1 | head -n 1 || true ;; ");
        script.push_str("*) (\"$t\" --version 2>&1 | head -n 1) || (\"$t\" -v 2>&1 | head -n 1) || true ;; ");
        script.push_str("esac; ");
        script.push_str("done; ");
        script.push_str("printf '\\n'; ");
    }

    if intent.want_fs {
        script.push_str("printf 'ls:\\n'; ls -1 2>/dev/null | head -n 40 || true; printf '\\n'; ");
    }

    if intent.want_git {
        script.push_str("printf 'git:\\n'; ");
        script.push_str("command -v git >/dev/null 2>&1 && ");
        script.push_str("git rev-parse --is-inside-work-tree 2>/dev/null && ");
        script.push_str("git status --porcelain=v1 -b 2>/dev/null | head -n 60 || true; ");
        script.push_str("printf '\\n'; ");
    }

    run_shell_script(&cfg, cwd, &script).await.unwrap_or_default()
}
