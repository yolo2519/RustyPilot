//! Parser module for processing AI responses.
//!
//! This module is responsible for parsing and structuring the responses
//! received from the AI service into usable command suggestions.

use super::client::AiCommandSuggestion;

/// Parse a command suggestion from AI response text.
///
/// Looks for patterns like:
/// ```
/// COMMAND: ls -la
/// EXPLANATION: Lists all files in long format
/// ALTERNATIVES: ls -lh, ll
/// ```
///
/// Also handles more natural responses where the command is in code blocks.
pub fn parse_command_suggestion(response: &str) -> Option<AiCommandSuggestion> {
    // Try structured format first
    if let Some(suggestion) = parse_structured_format(response) {
        return Some(suggestion);
    }

    // Try code block format
    if let Some(suggestion) = parse_code_block_format(response) {
        return Some(suggestion);
    }

    // Try inline command format
    parse_inline_format(response)
}

/// Parse structured format with COMMAND:, EXPLANATION:, ALTERNATIVES: labels
fn parse_structured_format(response: &str) -> Option<AiCommandSuggestion> {
    let mut command = None;
    let mut explanation = None;
    let mut alternatives = Vec::new();

    for line in response.lines() {
        let line = line.trim();
        
        if let Some(cmd) = line.strip_prefix("COMMAND:") {
            command = Some(cmd.trim().to_string());
        } else if let Some(exp) = line.strip_prefix("EXPLANATION:") {
            explanation = Some(exp.trim().to_string());
        } else if let Some(alt) = line.strip_prefix("ALTERNATIVES:") {
            // Parse comma-separated or newline-separated alternatives
            for alt_cmd in alt.split(',') {
                let trimmed = alt_cmd.trim();
                if !trimmed.is_empty() {
                    alternatives.push(trimmed.to_string());
                }
            }
        } else if !line.is_empty() && explanation.is_some() && command.is_some() {
            // Additional alternative commands on separate lines after ALTERNATIVES:
            if !line.starts_with("COMMAND:") && !line.starts_with("EXPLANATION:") {
                alternatives.push(line.to_string());
            }
        }
    }

    if let (Some(cmd), Some(exp)) = (command, explanation) {
        Some(AiCommandSuggestion {
            suggested_command: cmd,
            natural_language_explanation: exp,
            alternatives,
        })
    } else {
        None
    }
}

/// Parse commands from markdown code blocks
fn parse_code_block_format(response: &str) -> Option<AiCommandSuggestion> {
    // Look for ```bash or ``` code blocks
    let mut in_code_block = false;
    let mut command = String::new();
    let mut explanation_parts = Vec::new();
    let mut before_code = true;

    for line in response.lines() {
        let trimmed = line.trim();
        
        if trimmed.starts_with("```") {
            if in_code_block {
                // End of code block
                in_code_block = false;
                before_code = false;
            } else {
                // Start of code block
                in_code_block = true;
            }
        } else if in_code_block {
            // Inside code block - accumulate command
            if !command.is_empty() {
                command.push('\n');
            }
            command.push_str(trimmed);
        } else if !trimmed.is_empty() {
            // Outside code block - accumulate explanation
            if before_code {
                explanation_parts.push(trimmed.to_string());
            }
        }
    }

    if !command.is_empty() {
        let explanation = if explanation_parts.is_empty() {
            "Command suggested by AI".to_string()
        } else {
            explanation_parts.join(" ")
        };

        Some(AiCommandSuggestion {
            suggested_command: command.trim().to_string(),
            natural_language_explanation: explanation,
            alternatives: Vec::new(),
        })
    } else {
        None
    }
}

/// Parse inline format where command might be in backticks
fn parse_inline_format(response: &str) -> Option<AiCommandSuggestion> {
    // Look for text in backticks: `command here`
    let mut in_backticks = false;
    let mut command = String::new();
    let mut chars = response.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '`' {
            if in_backticks {
                // End of command
                if !command.is_empty() && is_likely_command(&command) {
                    return Some(AiCommandSuggestion {
                        suggested_command: command.trim().to_string(),
                        natural_language_explanation: extract_explanation(response, &command),
                        alternatives: Vec::new(),
                    });
                }
                command.clear();
                in_backticks = false;
            } else {
                in_backticks = true;
            }
        } else if in_backticks {
            command.push(ch);
        }
    }

    None
}

/// Check if a string looks like a shell command
fn is_likely_command(text: &str) -> bool {
    let text = text.trim();
    
    // Must have some content
    if text.is_empty() {
        return false;
    }

    // Common shell commands and patterns
    let common_commands = [
        "ls", "cd", "pwd", "echo", "cat", "grep", "find", "mkdir", "rm", "cp", "mv",
        "chmod", "chown", "ps", "kill", "top", "df", "du", "tar", "curl", "wget",
        "git", "npm", "cargo", "python", "node", "docker", "kubectl", "ssh", "scp",
    ];

    // Check if it starts with a common command
    for cmd in &common_commands {
        if text.starts_with(cmd) {
            return true;
        }
    }

    // Check for command-like patterns (has spaces, flags, etc.)
    text.contains(' ') && (text.contains('-') || text.contains('/'))
}

/// Extract explanation text from response, excluding the command itself
fn extract_explanation(response: &str, command: &str) -> String {
    // Remove the command from the response and use the rest as explanation
    let explanation = response.replace(&format!("`{}`", command), "")
        .replace(command, "")
        .trim()
        .to_string();

    if explanation.is_empty() {
        "Command suggested by AI".to_string()
    } else {
        // Take first sentence or first 200 chars
        explanation
            .lines()
            .next()
            .unwrap_or(&explanation)
            .chars()
            .take(200)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_structured_format() {
        let response = r#"
COMMAND: ls -la
EXPLANATION: Lists all files including hidden ones in long format
ALTERNATIVES: ls -lh, ll
        "#;

        let suggestion = parse_command_suggestion(response).unwrap();
        assert_eq!(suggestion.suggested_command, "ls -la");
        assert!(suggestion.natural_language_explanation.contains("Lists all files"));
        assert_eq!(suggestion.alternatives.len(), 2);
    }

    #[test]
    fn test_parse_code_block_format() {
        let response = r#"
You can list all files with:

```bash
ls -la
```

This shows all files including hidden ones.
        "#;

        let suggestion = parse_command_suggestion(response).unwrap();
        assert_eq!(suggestion.suggested_command, "ls -la");
    }

    #[test]
    fn test_parse_inline_format() {
        let response = "You should run `ls -la` to see all files.";

        let suggestion = parse_command_suggestion(response).unwrap();
        assert_eq!(suggestion.suggested_command, "ls -la");
    }
}
