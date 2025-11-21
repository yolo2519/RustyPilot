# RustyTerm Design Document

## 1. Overview
RustyTerm is a dual-pane terminal assistant composed of:
- A **real PTY-backed shell** (left pane)
- An **AI command copilot** (right pane)
- A **security layer** that evaluates AI-suggested commands
- **Context collection** (cwd, env, history)

Goal: Combine terminal productivity with safe AI-driven assistance.

---

## 2. Architecture Summary

```
+------------------- TUI (ratatui) -------------------+
| Left Pane  | Shell output + interactive PTY         |
| Right Pane | AI chat + command suggestions          |
+-----------------------------------------------------+
```

Modules:
- **shell/** — PTY, VT100 parsing, input handling
- **ai/** — OpenAI client, streaming, suggestions
- **ui/** — layout, panels, event loop
- **context/** — cwd, env, history snapshot
- **security/** — allowlist + analyzer
- **utils/** — logging

---

## 3. Subsystem Responsibilities

### Shell Subsystem
- Spawn shell inside PTY
- Stream output into VT100 parser
- Expose renderable lines to UI
- Handle input keys and window resize

### AI Subsystem
- Manage conversation history
- Stream model outputs to UI
- Produce structured `AiCommandSuggestion` objects

### Context Subsystem
- Collect runtime state:
  - Working directory
  - Environment variables
  - Recent commands

### Security Subsystem
- Detect dangerous suggestions
- Provide `Safe / Warn / Block` evaluation

### UI Subsystem
- Draw shell & AI panes
- Handle input focus + event loop
- Integrate async updates from shell & AI

---

## 4. Data Flow

### AI Suggestion
User Query → AI Panel → AiClient → JSON Suggestion → Security Check → (Optional) Execute via ShellManager

### Shell Rendering
PTY → reader thread → mpsc → VT100 parser → ShellPanel

---

## 5. Implementation Milestones

1. **M1 – Shell Pane**
   - PTY integration
   - Rendering + key forwarding
2. **M2 – AI Pane**
   - Chat UI + streaming
3. **M3 – Command Suggestions**
   - Structured JSON + prompt building
4. **M4 – Security Integration**
   - Warn/block commands before execution
5. **M5 – Context Snapshot**
   - Feed cwd/env/history into AI

