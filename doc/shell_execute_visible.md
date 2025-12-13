# ShellManager::execute_visible() Implementation

## Overview
Added a visible command injection API to `ShellManager` that allows programmatic execution of commands as if the user typed them into the terminal.

## Implementation

### Location
- **File**: `src/shell/subprocess.rs`
- **Method**: `pub fn execute_visible(&mut self, cmd: &str) -> anyhow::Result<()>`
- **Export**: Automatically available via `pub use subprocess::ShellManager` in `src/shell/mod.rs`

### Signature
```rust
pub fn execute_visible(&mut self, cmd: &str) -> Result<()>
```

### Behavior

#### What it does:
1. Acquires the PTY writer lock (with proper error context)
2. Writes the command string to the PTY
3. Appends a newline character (`\n`) to execute the command
4. Flushes the PTY writer to ensure immediate delivery
5. Returns immediately (non-blocking)

#### Non-blocking Design:
- Does NOT wait for command completion
- Output appears asynchronously via the existing PTY read loop
- Output is delivered through the `Receiver<Vec<u8>>` channel created in `ShellManager::new()`

### Error Handling
Uses `anyhow::Context` for rich error messages without `unwrap()`/`expect()`:

```rust
let mut pty_writer = self
    .pty_writer
    .lock()
    .map_err(|e| anyhow::anyhow!("Failed to lock PTY writer: {}", e))
    .context("Unable to acquire PTY writer lock for command execution")?;

pty_writer
    .write_all(cmd.as_bytes())
    .context("Failed to write command to PTY")?;

pty_writer
    .write_all(b"\n")
    .context("Failed to write newline to PTY")?;

pty_writer
    .flush()
    .context("Failed to flush PTY writer")?;
```

### Errors Returned
- PTY writer mutex is poisoned
- Writing to PTY fails (e.g., shell process exited)
- Flushing the PTY writer fails

## Documentation

### Doc Comment Sections
1. **Purpose**: Clear explanation of what the method does
2. **Non-blocking Behavior**: Emphasizes immediate return and asynchronous output
3. **Arguments**: Description of the `cmd` parameter
4. **Returns**: Success and error cases
5. **Example**: Practical usage example (marked `no_run`)
6. **Errors**: Comprehensive list of error conditions

### Usage Example
```rust
use rusty_term::shell::ShellManager;
use tokio::sync::mpsc::unbounded_channel;

let (tx, _) = unbounded_channel();
let (mut shell, mut rx) = ShellManager::new(tx, 80, 24)?;

// Execute a command
shell.execute_visible("ls -la")?;

// Command executes immediately, output comes asynchronously
while let Some(output) = rx.recv().await {
    // Process output bytes
}
```

## Integration Points

### Where to use:
This method is intended to be called when:
- AI suggests a command that the user confirms
- A security verdict is `Allow` or user confirms `RequireConfirmation`
- The application needs to execute a command programmatically

### Recommended call site:
In the event handler that processes command confirmations, likely:
- `src/event/assistant.rs` - When user presses Y/Enter to confirm a command
- `src/app.rs` - In the `AppEvent::ExecuteAiCommand` handler

### Example integration:
```rust
// In event handler when user confirms command
match verdict {
    Verdict::Allow | Verdict::RequireConfirmation => {
        // User confirmed, execute the command
        shell.execute_visible(&command)?;
    }
    Verdict::Deny => {
        // Only copy to clipboard, do not execute
    }
}
```

## Testing

### Unit Test Feasibility
Unit testing is challenging without spawning an actual PTY, which would require:
- Platform-specific PTY setup
- Subprocess management
- Async coordination

Instead, the implementation relies on:
1. **Comprehensive documentation** describing behavior and constraints
2. **Integration testing** by running the application
3. **Code review** of the implementation logic

### Manual Testing
To verify the implementation:
1. Start RustyTerm: `cargo run`
2. Use AI assistant to suggest a safe command (e.g., "list files")
3. Confirm the command (press Y)
4. Verify the command appears in the terminal and executes
5. Check that output appears correctly

### Code Quality Verification
```bash
# Compilation check
cargo check
✅ Finished `dev` profile

# Clippy with strict unwrap/expect checking
cargo clippy --bin rusty-term -- -D clippy::unwrap_used -D clippy::expect_used
✅ Exit code: 0 (no unwrap/expect violations)
```

## Comparison with inject_command()

The existing `inject_command()` method has similar functionality but:
- Uses simpler error messages
- Less comprehensive documentation
- `execute_visible()` is more explicit about its "visible" nature
- `execute_visible()` uses `anyhow::Context` for better error context

Both methods can coexist for different use cases:
- `inject_command()`: Internal/simple command injection
- `execute_visible()`: User-facing command execution with rich error context

## Files Modified
- `src/shell/subprocess.rs`
  - Added `use anyhow::Context`
  - Added `execute_visible()` method (~60 lines with documentation)

## Summary
✅ Method implemented with non-blocking behavior  
✅ No use of `unwrap()` or `expect()`  
✅ Rich error context using `anyhow::Context`  
✅ Comprehensive documentation with examples  
✅ Clippy passes with strict checking  
✅ Ready for integration with AI command execution flow




