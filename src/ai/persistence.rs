//! Disk persistence for AI sessions.
//!
//! Goal: keep AI sessions across program restarts, but still allow users to
//! manually close a session (which should delete it).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use super::client::AiCommandSuggestion;
use super::session::SessionId;

/// Persisted representation of a single chat message.
///
/// We intentionally keep this minimal and stable instead of serializing
/// async_openai types directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedMessage {
    pub role: String,   // "system" | "user" | "assistant"
    /// What we show in the UI (e.g. raw user input).
    pub content: String,
    /// What we send back to the model when replaying history.
    /// If None, `content` is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub id: SessionId,
    pub conversation: Vec<PersistedMessage>,
    pub last_suggestion: Option<AiCommandSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSessionState {
    pub version: u32,
    pub current_id: SessionId,
    pub next_id: SessionId,
    pub sessions: Vec<PersistedSession>,
}

pub fn default_sessions_path() -> PathBuf {
    // Simple, cross-platform default: ~/.rusty-term/sessions.json
    // (We can later switch to OS-native config dirs; keeping it dependency-free for now.)
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".rusty-term").join("sessions.json")
}

fn ensure_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create persistence directory: {}", parent.display()))?;
    }
    Ok(())
}

fn write_atomic(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    ensure_parent_dir(path)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data).with_context(|| format!("Failed to write temp file: {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "Failed to replace {} with {}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(())
}

pub fn load(path: &Path) -> anyhow::Result<PersistedSessionState> {
    let raw = fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let state: PersistedSessionState =
        serde_json::from_str(&raw).with_context(|| format!("Invalid session JSON at {}", path.display()))?;
    Ok(state)
}

pub fn save(path: &Path, state: &PersistedSessionState) -> anyhow::Result<()> {
    let data = serde_json::to_vec_pretty(state).context("Failed to serialize sessions")?;
    write_atomic(path, &data)
}


