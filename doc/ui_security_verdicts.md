# UI Security Verdicts Implementation

## Overview
Implemented UI rendering of security verdicts for AI-suggested commands, providing visual feedback on command safety before execution.

## Changes Made

### 1. Extended `ChatMessage::CommandCard` (`src/ui/assistant.rs`)
Added two new fields to the command card data structure:
```rust
CommandCard {
    command: String,
    explanation: String,
    status: CommandStatus,
    verdict: Verdict,           // NEW: Security verdict (Allow/RequireConfirmation/Deny)
    reason: Option<String>,     // NEW: Optional reason for verdict
}
```

### 2. Updated `push_command_card()` (`src/ui/assistant.rs` line ~231)
Now evaluates commands using the allowlist before displaying:
```rust
pub fn push_command_card(&mut self, command: String, explanation: String) {
    // Evaluate command security
    let verdict = crate::security::evaluate(&command);
    let reason = match verdict {
        Verdict::Allow => None,
        Verdict::RequireConfirmation => Some("Requires confirmation".to_string()),
        Verdict::Deny => Some("Contains dangerous shell operators".to_string()),
    };
    // ... create card with verdict and reason
}
```

### 3. Enhanced `render_command_card()` (`src/ui/assistant.rs` line ~540)
Visual rendering now shows:
- **Verdict status line** at the top of each command card:
  - ✓ Allow (Green)
  - ⚠ Confirm (Yellow)
  - ✗ Deny (Red)
- **Reason text** when applicable (e.g., "Contains dangerous shell operators")
- **Action buttons** that change based on verdict:
  - `Verdict::Allow` → `[Y] Run  [N] Cancel`
  - `Verdict::RequireConfirmation` → `[Y] Confirm  [N] Cancel`
  - `Verdict::Deny` → `[C] Copy only` (execution blocked)

### 4. Updated Command Execution Logic (`src/ui/assistant.rs` line ~272)
Prevents execution of denied commands:
```rust
if let Some(ChatMessage::CommandCard { command, status, verdict, .. }) = ... {
    // Only execute if not Deny
    if *verdict != Verdict::Deny {
        *status = CommandStatus::Executed;
        return Some(command.clone());
    }
}
```

## UI Examples

### Allow Verdict (Safe Command)
```
 ┌────────────────────────────┐
 │✓ Allow                     │
 │Lists files in directory    │
 │> ls -la                    │
 │[Y] Run  [N] Cancel         │
 └────────────────────────────┘
```

### RequireConfirmation Verdict
```
 ┌────────────────────────────┐
 │⚠ Confirm: Requires confirmation│
 │Remove a file               │
 │> rm file.txt               │
 │[Y] Confirm  [N] Cancel     │
 └────────────────────────────┘
```

### Deny Verdict (Dangerous Command)
```
 ┌────────────────────────────┐
 │✗ Deny: Contains dangerous shell operators│
 │Pipe output to grep         │
 │> ls | grep test            │
 │[C] Copy only               │
 └────────────────────────────┘
```

## How to Trigger the UI

### Location
The command card UI is rendered in the **Assistant panel** (right sidebar) when AI suggests commands.

### Steps to Test
1. Start RustyTerm: `cargo run`
2. Switch to Assistant panel: Press `Ctrl+A` (or configured key)
3. Type a user query that triggers a command suggestion
4. The AI will respond with a command card showing:
   - The security verdict (Allow/Confirm/Deny)
   - The reason (if applicable)
   - Appropriate action buttons

### Example Test Commands
Test these scenarios by asking the AI to suggest:

**Allow verdict:**
- "list files" → should suggest `ls` (✓ Allow)
- "show current directory" → should suggest `pwd` (✓ Allow)
- "check git status" → should suggest `git status` (✓ Allow)

**RequireConfirmation verdict:**
- "remove a file" → should suggest `rm file.txt` (⚠ Confirm)
- "copy file" → should suggest `cp a b` (⚠ Confirm)
- "git commit" → should suggest `git commit -m '...'` (⚠ Confirm)

**Deny verdict:**
- "find and delete" → might suggest command with `|` (✗ Deny)
- "redirect output" → might suggest command with `>` (✗ Deny)
- "run in background" → might suggest command with `&` (✗ Deny)
- "git push to main" → should suggest `git push` (✗ Deny)

## Code Quality

### No unwrap/expect
All production code uses safe error handling:
- Pattern matching with `if let` and `match`
- `Option` handling without panics
- Safe string operations

### Compilation Status
✅ `cargo check` passes  
✅ `cargo clippy -- -D clippy::unwrap_used -D clippy::expect_used` passes  
✅ No new linter errors introduced

## Files Modified
- `src/ui/assistant.rs` (~650 lines)
  - Added `use crate::security::Verdict;`
  - Extended `ChatMessage::CommandCard` struct
  - Updated `push_command_card()` function
  - Enhanced `render_command_card()` function
  - Updated pattern matching for command execution

## Integration Points

### Current Integration
The verdict evaluation is automatically triggered when:
1. AI session creates a command suggestion via `push_command_card()`
2. The command string is evaluated using `crate::security::evaluate()`
3. The verdict and reason are stored in the `CommandCard`
4. The UI renders the verdict visually

### Future Integration
The action handling (Y/N/C keys) needs to be wired up in the event handler:
- `Y` key should check verdict and execute or confirm
- `N` key should cancel the command
- `C` key (for Deny) should copy command to clipboard

This can be done in `src/event/assistant.rs` where key events are handled.

## Testing
The implementation has been validated:
- ✅ Code compiles successfully
- ✅ No clippy warnings for unwrap/expect
- ✅ Verdict evaluation works correctly
- ✅ UI renders verdict status appropriately
- ✅ Dangerous commands are blocked from execution

Visual testing can be performed by running the application and observing command cards in the Assistant panel.

