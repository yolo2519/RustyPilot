# AI Session Manager Documentation

## Overview

The AI Session Manager is a core component of RustyTerm that handles communication with OpenAI's API, manages conversation history, streams responses in real-time, and parses command suggestions.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     AI Session Manager                       │
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   Session 1  │  │   Session 2  │  │   Session N  │      │
│  │              │  │              │  │              │      │
│  │ - History    │  │ - History    │  │ - History    │      │
│  │ - Suggestion │  │ - Suggestion │  │ - Suggestion │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                               │
│  ┌───────────────────────────────────────────────────┐      │
│  │         OpenAI Client (async_openai)              │      │
│  └───────────────────────────────────────────────────┘      │
└─────────────────────────────────────────────────────────────┘
         │                                    │
         │ AiStreamData                       │ AppEvent
         │ (high-frequency)                   │ (low-frequency)
         ▼                                    ▼
   ┌──────────┐                        ┌──────────┐
   │ UI/TUI   │                        │   App    │
   │ Component│                        │  Logic   │
   └──────────┘                        └──────────┘
```

## Key Components

### 1. AiSessionManager

The main manager that:
- Maintains multiple chat sessions
- Routes messages to OpenAI
- Handles streaming responses
- Manages conversation history

**Key Methods:**
- `new(ai_stream_tx, app_event_tx, model)` - Create a new manager
- `send_message(session_id, user_input, context)` - Send a message with context
- `new_session()` - Create a new chat session
- `switch_session(session_id)` - Switch between sessions
- `get_last_suggestion(session_id)` - Get the last command suggestion
- `execute_suggestion(session_id)` - Execute a suggested command

### 2. AiSession

Represents a single conversation thread:
- `conversation_history` - Full OpenAI message history
- `last_suggestion` - Most recent command suggestion
- `current_response` - Accumulating response text during streaming

### 3. Communication Channels

#### AI Stream Channel (Bounded, High-Frequency)
Used for streaming AI responses:
```rust
pub enum AiStreamData {
    Chunk { session_id, text },      // Incremental text
    End { session_id },               // Stream completed
    Error { session_id, error },      // Error occurred
}
```

#### App Event Channel (Unbounded, Low-Frequency)
Used for coordination events:
```rust
pub enum AppEvent {
    AiCommandSuggestion { session_id, command, explanation },
    ExecuteAiCommand { session_id },
    // ... other events
}
```

## Usage Example

### Basic Setup

```rust
use rusty_term::ai::AiSessionManager;
use rusty_term::context::ContextSnapshot;
use rusty_term::event::{init_app_eventsource, AiStreamData};
use tokio::sync::mpsc;

// Create channels
let (ai_stream_tx, mut ai_stream_rx) = mpsc::channel(100);
let (app_event_tx, mut app_event_rx) = init_app_eventsource();

// Create manager
let mut manager = AiSessionManager::new(
    ai_stream_tx,
    app_event_tx,
    "gpt-4o-mini"
);
```

### Sending a Message

```rust
// Build context
let context = ContextSnapshot {
    cwd: "/home/user/project".to_string(),
    env_vars: vec![
        ("HOME".to_string(), "/home/user".to_string()),
    ],
    recent_history: vec![
        "cargo build".to_string(),
        "git status".to_string(),
    ],
};

// Send message
let session_id = manager.current_session_id();
manager.send_message(session_id, "list all rust files", context);
```

### Processing Responses

```rust
// In your event loop
loop {
    tokio::select! {
        // Handle streaming chunks
        Some(stream_data) = ai_stream_rx.recv() => {
            match stream_data {
                AiStreamData::Chunk { session_id, text } => {
                    // Display chunk in UI
                    print!("{}", text);
                    manager.append_chunk(session_id, &text);
                }
                AiStreamData::End { session_id } => {
                    // Finalize the response
                    let response = manager.get_current_response(session_id)
                        .unwrap_or("")
                        .to_string();
                    manager.finalize_response(session_id, response);
                }
                AiStreamData::Error { error, .. } => {
                    eprintln!("Error: {}", error);
                }
            }
        }

        // Handle command suggestions
        Some(app_event) = app_event_rx.recv() => {
            match app_event {
                AppEvent::AiCommandSuggestion { command, explanation, .. } => {
                    println!("Suggested: {}", command);
                    println!("Because: {}", explanation);
                }
                _ => {}
            }
        }
    }
}
```

## Response Parsing

The AI Session Manager automatically parses command suggestions from AI responses. It supports multiple formats:

### 1. Structured Format
```
COMMAND: ls -la
EXPLANATION: Lists all files including hidden ones
ALTERNATIVES: ls -lh, ll
```

### 2. Code Block Format
```markdown
You can list files with:

```bash
ls -la
```

This shows all files including hidden ones.
```

### 3. Inline Format
```
You should run `ls -la` to see all files.
```

## Context Integration

The manager automatically includes system context in prompts:

- **Current Directory**: Where the user is working
- **Environment Variables**: HOME, SHELL, USER, etc.
- **Command History**: Recent commands for context
- **User Query**: The natural language request

Example prompt sent to AI:
```
USER REQUEST:
list all rust files

CURRENT DIRECTORY:
/home/user/project

RECENT COMMAND HISTORY:
  1. git status
  2. cargo build

RELEVANT ENVIRONMENT:
  HOME=/home/user
  SHELL=/bin/bash

Please suggest a shell command to accomplish this task.
Format your response with:
COMMAND: <the command>
EXPLANATION: <what it does>
ALTERNATIVES: <optional alternatives>
```

## Session Management

### Creating Multiple Sessions

```rust
// Create a new session
let session_id = manager.new_session();

// Switch between sessions
manager.switch_session(1);  // Back to first session
manager.switch_session(session_id);  // To new session
```

### Getting Session Information

```rust
// Current session
let session = manager.current_session().unwrap();
println!("History length: {}", session.conversation_history.len());

// Last suggestion
if let Some(suggestion) = manager.get_last_suggestion(session_id) {
    println!("Command: {}", suggestion.suggested_command);
    println!("Explanation: {}", suggestion.natural_language_explanation);
}
```

## Error Handling

The manager handles errors gracefully:

1. **Network Errors**: Sent as `AiStreamData::Error`
2. **Parsing Errors**: Logged, but don't crash
3. **Invalid Sessions**: Return None or false
4. **API Errors**: Forwarded through error channel

Example error handling:
```rust
match stream_data {
    AiStreamData::Error { error, session_id } => {
        eprintln!("Session {} error: {}", session_id, error);
        // Update UI to show error state
    }
    _ => {}
}
```

## Configuration

### Model Selection

```rust
// Use different models
let manager = AiSessionManager::new(
    ai_stream_tx,
    app_event_tx,
    "gpt-4o-mini"  // Fast and cheap
    // "gpt-4"      // More capable
    // "gpt-3.5-turbo"  // Older, cheaper
);
```

### Custom System Prompt

The default system prompt can be customized by modifying `AiSessionManager::default_system_prompt()`.

## Performance Considerations

1. **Streaming**: Responses stream in real-time for better UX
2. **Channel Sizes**: AI stream channel is bounded (100) to prevent memory issues
3. **Async Tasks**: Each request spawns a separate tokio task
4. **History Management**: Full conversation history is kept for context

## Testing

Run the demo:
```bash
export OPENAI_API_KEY='your-key-here'
cargo run --example ai_session_demo
```

Run unit tests:
```bash
cargo test --package rusty-term --lib ai
```

## Integration with Shell Manager

The AI Session Manager is designed to work alongside the Shell Manager:

1. User types natural language request in Assistant panel
2. AI Session Manager sends to OpenAI with context
3. Response streams back to Assistant panel
4. Command suggestion is parsed and displayed
5. User can accept suggestion
6. Shell Manager executes the command
7. Output appears in Terminal panel

## Security Considerations

1. **API Key**: Store in environment variable, never in code
2. **Command Validation**: Always show commands before execution
3. **Dangerous Commands**: AI is prompted to warn about risky operations
4. **User Confirmation**: Never auto-execute without user approval

## Future Enhancements

- [ ] Token usage tracking
- [ ] Cost estimation
- [ ] Response caching
- [ ] Custom model configurations
- [ ] Conversation export/import
- [ ] Multi-turn command refinement
- [ ] Command safety scoring

