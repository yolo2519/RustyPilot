# AI Session Manager - Quick Start Guide

## What is it?

The AI Session Manager handles all communication with OpenAI to provide intelligent command suggestions based on natural language input and system context.

## Key Features

✅ **Real-time Streaming** - See AI responses as they're generated  
✅ **Context-Aware** - Includes current directory, environment, and command history  
✅ **Multi-Session** - Maintain multiple independent conversations  
✅ **Smart Parsing** - Automatically extracts command suggestions from responses  
✅ **Safe Execution** - Commands require user confirmation  

## Quick Example

```rust
use rusty_term::ai::AiSessionManager;
use rusty_term::context::ContextSnapshot;
use rusty_term::event::{init_app_eventsource, AiStreamData};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    // 1. Set up channels
    let (ai_stream_tx, mut ai_stream_rx) = mpsc::channel(100);
    let (app_event_tx, mut app_event_rx) = init_app_eventsource();

    // 2. Create manager
    let mut manager = AiSessionManager::new(
        ai_stream_tx,
        app_event_tx,
        "gpt-4o-mini"
    );

    // 3. Build context
    let context = ContextSnapshot {
        cwd: "/home/user/project".to_string(),
        env_vars: vec![],
        recent_history: vec![],
    };

    // 4. Send message
    let session_id = manager.current_session_id();
    manager.send_message(session_id, "list all rust files", context);

    // 5. Process response
    while let Some(data) = ai_stream_rx.recv().await {
        match data {
            AiStreamData::Chunk { text, .. } => print!("{}", text),
            AiStreamData::End { session_id } => {
                let response = manager.get_current_response(session_id)
                    .unwrap_or("")
                    .to_string();
                manager.finalize_response(session_id, response);
                break;
            }
            AiStreamData::Error { error, .. } => {
                eprintln!("Error: {}", error);
                break;
            }
        }
    }
}
```

## How It Works

```
User Input → Session Manager → OpenAI API
                ↓
         Build Prompt with:
         • User query
         • Current directory
         • Environment vars
         • Command history
                ↓
         Stream Response → Parse Commands → Display to User
```

## Response Formats Supported

### Structured
```
COMMAND: ls -la
EXPLANATION: Lists all files
ALTERNATIVES: ls -lh
```

### Code Block
````markdown
```bash
ls -la
```
````

### Inline
```
Run `ls -la` to list files.
```

## Running the Demo

```bash
# Set your API key
export OPENAI_API_KEY='sk-...'

# Run the demo
cargo run --example ai_session_demo

# Try commands like:
# - "list all rust files"
# - "find files modified today"
# - "show git status"
```

## Integration Points

### From Your App
```rust
// Send user request
manager.send_message(session_id, user_input, context);
```

### To Your UI
```rust
// Receive streaming chunks
match ai_stream_rx.recv().await {
    Some(AiStreamData::Chunk { text, .. }) => {
        // Update UI with new text
    }
    Some(AiStreamData::End { .. }) => {
        // Response complete
    }
    _ => {}
}
```

### To Your Shell
```rust
// Receive command suggestions
match app_event_rx.recv().await {
    Some(AppEvent::AiCommandSuggestion { command, .. }) => {
        // Display command to user for approval
    }
    Some(AppEvent::ExecuteAiCommand { session_id }) => {
        // User approved - execute the command
    }
    _ => {}
}
```

## Architecture Diagram (from your sketch)

```
┌─────────────────────────────────────────┐
│              App (UI)                    │
│  ┌──────────┐         ┌──────────┐     │
│  │ Terminal │         │ Assistant│     │
│  └──────────┘         └──────────┘     │
└─────────────────────────────────────────┘
         │                     │
         │ pty output          │ UI update
         │                     │
    ┌────▼─────┐         ┌────▼──────────┐
    │  Shell   │         │ AI Session    │
    │ Manager  │◄────────┤ Manager       │
    └──────────┘ command │               │
         │      suggestion└───────────────┘
         │                     │
         │ pty write           │ network
         ▼                     ▼
    ┌──────────┐         ┌──────────┐
    │  Shell   │         │ OpenAI   │
    │ (bash)   │         │ API      │
    └──────────┘         └──────────┘
```

## Configuration

### Model Selection
```rust
// Fast and cheap
AiSessionManager::new(tx, tx, "gpt-4o-mini")

// More capable
AiSessionManager::new(tx, tx, "gpt-4o")

// Most powerful
AiSessionManager::new(tx, tx, "gpt-4")
```

### Environment Variables
```bash
# Required
export OPENAI_API_KEY='your-key-here'

# Optional (uses default if not set)
export OPENAI_API_BASE='https://api.openai.com/v1'
```

## Error Handling

All errors are sent through the `AiStreamData::Error` channel:

```rust
AiStreamData::Error { session_id, error } => {
    // Network error
    // API error
    // Parsing error
    // etc.
}
```

## Testing

```bash
# Run unit tests
cargo test --package rusty-term --lib ai

# Run integration test
cargo run --example ai_session_demo
```

## Files Modified/Created

### Core Implementation
- `src/ai/session.rs` - Main session manager (✨ **completely rewritten**)
- `src/ai/client.rs` - OpenAI client wrapper (✨ **enhanced**)
- `src/ai/parser.rs` - Response parsing (✨ **new implementation**)
- `src/ai/prompt.rs` - Context-aware prompts (✨ **enhanced**)

### Documentation & Examples
- `examples/ai_session_demo.rs` - Interactive demo (✨ **new**)
- `doc/ai_session_manager.md` - Full documentation (✨ **new**)
- `AI_SESSION_MANAGER_QUICKSTART.md` - This file (✨ **new**)

## Next Steps

1. ✅ AI Session Manager implemented
2. ⏭️ Integrate with Shell Manager
3. ⏭️ Build UI components
4. ⏭️ Add command approval flow
5. ⏭️ Implement safety checks

## Need Help?

- See `doc/ai_session_manager.md` for detailed documentation
- Run `cargo run --example ai_session_demo` for interactive demo
- Check `examples/tui_chat_assistant.rs` for UI patterns

