# RustyTerm Architecture Flow

## Complete System Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           USER INTERFACE (TUI)                           │
│                                                                           │
│  ┌──────────────────────────┐       ┌──────────────────────────────┐   │
│  │    Terminal Panel        │       │    Assistant Panel           │   │
│  │  ┌────────────────────┐  │       │  ┌────────────────────────┐ │   │
│  │  │ $ ls -la           │  │       │  │ You: list rust files   │ │   │
│  │  │ file1.rs           │  │       │  │                        │ │   │
│  │  │ file2.rs           │  │       │  │ AI: To list all Rust   │ │   │
│  │  │ $ _                │  │       │  │ files, use:            │ │   │
│  │  └────────────────────┘  │       │  │                        │ │   │
│  │                           │       │  │ ┌────────────────────┐ │ │   │
│  └──────────────────────────┘       │  │ │ find . -name '*.rs'│ │ │   │
│                                      │  │ │ [Y] Execute [N] No │ │ │   │
│                                      │  │ └────────────────────┘ │ │   │
│                                      │  └────────────────────────┘ │   │
│                                      └──────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                    │                                    │
                    │ PTY Output                         │ User Input
                    │ (stdout/stderr)                    │ (natural language)
                    ▼                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                          APPLICATION LAYER                               │
│                                                                           │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                         App State                                │   │
│  │  - Active Pane (Terminal/Assistant)                              │   │
│  │  - Command Mode                                                  │   │
│  │  - Event Routing                                                 │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                    │                                    │                │
│                    │ PTY Data                           │ AI Request     │
│                    ▼                                    ▼                │
│  ┌──────────────────────────┐       ┌──────────────────────────────┐   │
│  │    Shell Manager         │       │    AI Session Manager        │   │
│  │                          │       │                              │   │
│  │  - PTY Management        │       │  - Session Management        │   │
│  │  - Command Injection     │       │  - Conversation History      │   │
│  │  - Output Streaming      │◄──────┤  - Command Suggestions       │   │
│  │                          │ exec  │  - Response Parsing          │   │
│  └──────────────────────────┘       └──────────────────────────────┘   │
│                    │                                    │                │
└────────────────────┼────────────────────────────────────┼────────────────┘
                     │                                    │
                     │ pty_write                          │ HTTPS
                     ▼                                    ▼
┌──────────────────────────┐       ┌──────────────────────────────┐
│    Shell Process         │       │    OpenAI API                │
│    (bash/zsh/fish)       │       │    (GPT-4o-mini)             │
│                          │       │                              │
│  $ find . -name '*.rs'   │       │  Streaming Response:         │
│  ./src/main.rs           │       │  "COMMAND: find . -name..."  │
│  ./src/lib.rs            │       │                              │
└──────────────────────────┘       └──────────────────────────────┘
```

## Event Flow Diagram

### 1. User Types in Assistant Panel

```
User Input: "list all rust files"
     │
     ▼
┌─────────────────────────────────────────┐
│ Event Handler (assistant.rs)            │
│ - Captures input                         │
│ - Builds ContextSnapshot                 │
│ - Calls ai_sessions.send_message()       │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ AI Session Manager                       │
│ - Adds user message to history           │
│ - Builds prompt with context             │
│ - Sends to OpenAI API                    │
│ - Spawns async streaming task            │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ OpenAI API (Streaming)                   │
│ - Processes request                      │
│ - Streams response tokens                │
└─────────────────────────────────────────┘
     │
     │ (multiple chunks)
     ▼
┌─────────────────────────────────────────┐
│ AI Stream Channel                        │
│ AiStreamData::Chunk { text: "To list" } │
│ AiStreamData::Chunk { text: " all" }    │
│ AiStreamData::Chunk { text: " Rust" }   │
│ ...                                      │
│ AiStreamData::End { session_id }        │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ TuiAssistant (UI Component)              │
│ - Accumulates chunks                     │
│ - Updates display in real-time           │
│ - On End: parses command suggestion      │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ Response Parser                          │
│ - Detects format (structured/code/inline)│
│ - Extracts command                       │
│ - Extracts explanation                   │
│ - Extracts alternatives                  │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ App Event Channel                        │
│ AppEvent::AiCommandSuggestion {          │
│   session_id,                            │
│   command: "find . -name '*.rs'",        │
│   explanation: "Lists all Rust files"    │
│ }                                        │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ App Event Handler                        │
│ - Displays command card in UI            │
│ - Waits for user confirmation            │
└─────────────────────────────────────────┘
```

### 2. User Confirms Command (Press Y)

```
User Input: Y
     │
     ▼
┌─────────────────────────────────────────┐
│ Event Handler (assistant.rs)            │
│ - Detects confirmation                   │
│ - Calls ai_sessions.execute_suggestion() │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ AI Session Manager                       │
│ - Retrieves last suggestion              │
│ - Sends ExecuteAiCommand event           │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ App Event Channel                        │
│ AppEvent::ExecuteAiCommand {             │
│   session_id                             │
│ }                                        │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ App Event Handler                        │
│ - Gets command from ai_sessions          │
│ - Calls shell_manager.inject_command()   │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ Shell Manager                            │
│ - Writes command to PTY                  │
│ - Simulates Enter key                    │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ Shell Process                            │
│ - Executes: find . -name '*.rs'          │
│ - Outputs results                        │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ PTY Output Channel                       │
│ - Streams output back to UI              │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ TuiTerminal (UI Component)               │
│ - Displays output in Terminal Panel      │
│ ./src/main.rs                            │
│ ./src/lib.rs                             │
│ ...                                      │
└─────────────────────────────────────────┘
```

## Channel Architecture

### High-Frequency Channels (Bounded)

```
┌─────────────────────────────────────────┐
│ AI Stream Channel (capacity: 256)       │
│                                          │
│ Purpose: Stream AI response chunks       │
│ Producer: OpenAI streaming task          │
│ Consumer: TuiAssistant                   │
│                                          │
│ Data Types:                              │
│ - Chunk { session_id, text }             │
│ - End { session_id }                     │
│ - Error { session_id, error }            │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│ PTY Output Channel (capacity: 1024)      │
│                                          │
│ Purpose: Stream shell output             │
│ Producer: PTY reader task                │
│ Consumer: TuiTerminal                    │
│                                          │
│ Data Types:                              │
│ - Vec<u8> (raw terminal output)          │
└─────────────────────────────────────────┘
```

### Low-Frequency Channels (Unbounded)

```
┌─────────────────────────────────────────┐
│ App Event Channel (unbounded)            │
│                                          │
│ Purpose: Coordinate between components   │
│ Producers: Multiple (AI, Shell, etc.)    │
│ Consumer: App main loop                  │
│                                          │
│ Event Types:                             │
│ - AiCommandSuggestion                    │
│ - ExecuteAiCommand                       │
│ - ShellError                             │
│ - ShellCommandCompleted                  │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│ User Event Channel (capacity: 64)        │
│                                          │
│ Purpose: User keyboard/mouse input       │
│ Producer: Crossterm event reader         │
│ Consumer: App main loop                  │
│                                          │
│ Event Types:                             │
│ - Key events                             │
│ - Mouse events                           │
│ - Resize events                          │
└─────────────────────────────────────────┘
```

## Context Flow

```
┌─────────────────────────────────────────┐
│ Context Manager                          │
│                                          │
│ Tracks:                                  │
│ - Current working directory              │
│ - Environment variables                  │
│ - Command history                        │
└─────────────────────────────────────────┘
     │
     │ snapshot()
     ▼
┌─────────────────────────────────────────┐
│ ContextSnapshot (Immutable)              │
│                                          │
│ Contains:                                │
│ - cwd: String                            │
│ - env_vars: Vec<(String, String)>        │
│ - recent_history: Vec<String>            │
└─────────────────────────────────────────┘
     │
     │ passed to
     ▼
┌─────────────────────────────────────────┐
│ Prompt Builder                           │
│                                          │
│ Builds:                                  │
│ USER REQUEST: <query>                    │
│ CURRENT DIRECTORY: <cwd>                 │
│ RECENT HISTORY: <commands>               │
│ ENVIRONMENT: <vars>                      │
└─────────────────────────────────────────┘
     │
     │ sent to
     ▼
┌─────────────────────────────────────────┐
│ OpenAI API                               │
│                                          │
│ Returns context-aware suggestions        │
└─────────────────────────────────────────┘
```

## Session Management

```
┌─────────────────────────────────────────────────────────────┐
│ AI Session Manager                                           │
│                                                               │
│  sessions: HashMap<SessionId, AiSession>                     │
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  Session 1   │  │  Session 2   │  │  Session 3   │      │
│  │              │  │              │  │              │      │
│  │ History:     │  │ History:     │  │ History:     │      │
│  │ - System msg │  │ - System msg │  │ - System msg │      │
│  │ - User: "ls" │  │ - User: "git"│  │ - User: "..."│      │
│  │ - AI: "..."  │  │ - AI: "..."  │  │              │      │
│  │              │  │              │  │              │      │
│  │ Suggestion:  │  │ Suggestion:  │  │ Suggestion:  │      │
│  │ ls -la       │  │ git status   │  │ None         │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│         ▲                                                     │
│         │ current_id = 1                                     │
└─────────┼─────────────────────────────────────────────────────┘
          │
          │ User can switch with Tab/Shift+Tab
          │
```

## Error Handling Flow

```
┌─────────────────────────────────────────┐
│ Error Source                             │
│ - Network timeout                        │
│ - API error (rate limit, invalid key)    │
│ - Parse failure                          │
│ - Invalid session                        │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ Error Handler                            │
│ - Logs error                             │
│ - Sends AiStreamData::Error              │
│ - Updates UI state                       │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ UI Display                               │
│ - Shows error message in red             │
│ - Allows user to retry                   │
│ - Session remains intact                 │
└─────────────────────────────────────────┘
```

## Key Design Principles

1. **Separation of Concerns**
   - UI components don't know about OpenAI
   - AI manager doesn't know about rendering
   - Clear interfaces between layers

2. **Non-Blocking Architecture**
   - All I/O is async
   - UI remains responsive during AI requests
   - Streaming prevents long waits

3. **Type Safety**
   - Strong typing prevents errors
   - Enums for event types
   - Compile-time guarantees

4. **Error Recovery**
   - Graceful degradation
   - Clear error messages
   - Session state preserved

5. **Extensibility**
   - Easy to add new parsers
   - Support for different models
   - Pluggable components


