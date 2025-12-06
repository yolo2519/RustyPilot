//! Current working directory tracking.
//!
//! This module captures and tracks the current working directory
//! of the shell session, providing context for AI command suggestions.

use std::env;

#[derive(Clone, Default, Debug)]
pub struct CurrentDir {
    pub path: String,
}

impl CurrentDir {
    /// Capture the current working directory from the process.
    pub fn capture() -> Option<Self> {
        let path = env::current_dir().ok()?.to_string_lossy().to_string();
        Some(Self { path })
    }

    /// Update the current working directory.
    pub fn update(&mut self, new_path: String) {
        self.path = new_path;
    }

    /// Update from an OSC 7 sequence (file://host/path format).
    /// Returns true if successfully parsed and updated.
    pub fn update_from_osc7(&mut self, osc_payload: &str) -> bool {
        // OSC 7 format: file://hostname/path
        if let Some(path) = osc_payload.strip_prefix("file://") {
            // Skip hostname (everything before the first /)
            if let Some(idx) = path.find('/') {
                let decoded_path = &path[idx..];
                // URL decode the path (handle %XX sequences)
                if let Ok(decoded) = urlencoding_decode(decoded_path) {
                    self.path = decoded;
                    return true;
                }
            }
        }
        false
    }
}

/// Simple URL decoding for path strings.
fn urlencoding_decode(s: &str) -> Result<String, ()> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                    continue;
                }
            }
            return Err(());
        } else {
            result.push(c);
        }
    }
    Ok(result)
}
