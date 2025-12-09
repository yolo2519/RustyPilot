# Cleanup Legacy Text Parser Code

## Background

After implementing Tool Calling, the following code is no longer used but kept temporarily for reference.

## Files to Clean Up

### 1. `src/ai/parser.rs`

**Status**: No longer used by `session.rs`

**Contents**:
- `parse_command_suggestion()` - Parse command suggestions from AI text responses
- `parse_structured_format()` - Parse `COMMAND: xxx` format
- `parse_code_block_format()` - Parse markdown code block format
- `parse_inline_format()` - Parse inline command format

**Dependencies**: Uses `AiCommandSuggestion` struct from `client.rs`

### 2. `src/ai/client.rs`

**Status**: Only used by `parser.rs`

**Contents**:
- `AiCommandSuggestion` struct - Legacy command suggestion data structure
- `AiClient` - Legacy non-streaming AI client
- `send_request()` - Non-streaming request method

## New Implementation Location

Tool Calling related code is now located in:

- `src/ai/session.rs`:
  - `SuggestCommandArgs` - New command suggestion args (parsed from tool call JSON)
  - `CommandSuggestionRecord` - Command suggestion record (includes tool_call_id)
  - `create_suggest_command_tool()` - Tool definition
  - `process_tool_calls()` - Handle tool calls

- `src/event/mod.rs`:
  - `AiStreamData::ToolCalls` - Streaming tool calls data
  - `AiUiUpdate::CommandSuggestion` - UI update event

## Cleanup Steps

1. Verify `parser.rs` is not referenced elsewhere
2. Verify `AiCommandSuggestion` and related methods in `client.rs` are not referenced elsewhere
3. Remove `parser` and related exports from `src/ai/mod.rs`
4. Delete or comment out legacy code in `parser.rs` and `client.rs`
5. Update re-exports in `lib.rs` (if any)

## Notes

- If fallback to text parsing is needed in the future (e.g., some models don't support tool calling), this code can be restored
- Test cases in `parser.rs` can be referenced to understand the legacy parsing logic
