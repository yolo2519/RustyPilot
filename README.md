# RustyTerm: Rust-Based AI Copilot for the Command Line

## **Team Information**


| Member      | Student Number | Email                        |
|-------------|----------------|------------------------------|
| Weijie Zhu  | 1009310906     | weijie.zhu@mail.utoronto.ca  |
| Irys Zhang  | 1012794424     | irys.zhang@mail.utoronto.ca  |
| Yushun Tang | 1011561962     | yushun.tang@mail.utoronto.ca |

---

## **Video Slide Presentation**

_Add link here._

## **Video Demo**

_Add link here._

## **Motivation**

In the modern software development workflow, the command line interface remains one of the most powerful tools for interacting with systems. Despite its power, many users struggle with recalling the correct command syntax, flags, and pipelines that are essential to accomplish common developer tasks. In practice, developers often know what they want to do, but not how to do it using the CLI. This leads many developers, both novice and experienced, to constantly switch between the terminal and other applications like web browsers, search engines, or external AI tools just to remember the right commands. These context switches break focus, slow down productivity, and disrupt the cognitive flow of problem solving.

This real world pain point was the primary motivation for building Rusty-Term, a Rust-based terminal tool designed to address this inefficiency by integrating AI command suggestions directly inside a terminal environment. Many common shell tasks, such as file search, text processing, and directory management, require memorizing complex command flags or repeatedly searching for examples online. Rusty-Term was conceived to bring the power of modern AI command assistance into the shell in a fluid and interactive way.

Instead of copying and pasting between terminals and AI chat tools, Rusty-Term aims to make the command line itself a smart assistant. By doing so, we hoped to preserve workflow continuity, reduce friction, and ultimately empower users to accomplish shell tasks more efficiently without leaving the terminal.

Beyond improving usability, this project also filled a gap in the Rust ecosystem. While there are certain AI-assisted developer tools, there are very few Rust-based interactive command line assistants that combine a live shell session and AI suggestions in a unified environment. Building this tool offered an opportunity for us to explore the potential of embedding AI into traditionally low-UI environments, pushing forward the conversation around human-AI interaction in command line workflows.

Rust as a language was a deliberate choice due to its performance, safety guarantees, and ecosystem strengths such as robust concurrency and TUI libraries. The project allowed us to explore advanced Rust features such as asynchronous programming, subprocess management, and cross-platform terminal control in a real world setting.

Overall, Rusty-Term’s motivation was to reduce cognitive overhead, streamline developer workflows, and showcase how modern AI can be embedded into even the most traditional developer tools without sacrificing control, transparency, or safety.

## **Objectives**

The primary objective of this project was to design and implement a Rust-based AI assistant for shell environments that would provide users with intelligent and context-aware command recommendations without leaving the terminal. To accomplish this, we set several concrete goals:

- **Improve Developer Efficiency**: By reducing time spent switching between applications, Rusty-Term aims to keep users focused within the terminal by providing AI-driven command suggestions that are relevant to a user’s intent.
- **Context Awareness**: Develop a system where the assistant understands relevant context like working directory, environment variables, and previous command history to tailor command suggestions.
- **Interactive Command Assistance**: Allow users not only to receive suggestions but also to interact with them: understand what the command does, get explanations, accept them for execution, or ask for refinements.
- **Security and Safety**: Implement a safety layer that prevents harmful operations from being automatically executed by analyzing AI-suggested commands before execution.
- **Robust and Responsive Interface**: Create a split-screen TUI that feels natural for interactive use, enabling fast keyboard-based navigation and input.
- **Modular and Extensible Design**: Architect the system in a modular way so future features could be added, such as editor integration, cloud-based AI models, logging, or more advanced contextual intelligence.
- **Rust Ecosystem Contribution**: Use the Rust ecosystem to build a reliable, safe, and high performance tool that may serve as a building block or inspiration for future Rust-based developer tools.

The successful completion of these objectives required integrating AI APIs, handling asynchronous operations, rendering terminal user interfaces, analyzing user input and command results, enforcing security policies, and managing subprocesses; these were central technical objectives throughout the project’s implementation.

## **Features**

Rusty-Term’s final deliverable offers a series of features that together form a usable AI-assisted shell experience. These are grouped by core functionality and interaction:

### i). TUI Split Interface

At the core of Rusty-Term is its interactive text-based user interface. This interface consists of two main panes:

- **Left Pane**: A fully functional shell session, typically Zsh or Bash based on the user's `$SHELL` configuration, where the user can type and execute commands as normal.
- **Right Pane**: The AI assistant panel where users enter natural language queries and receive suggested commands.

The split layout is navigable entirely via keyboard or mouse, preserving the feel of modern terminal workflows.

### ii). AI Command Suggestions

The tool supports two primary actions for each AI-generated command:

- **Execute / Copy**: Accept the suggestion. If the command is evaluated as low or medium risk, it is executed directly in the shell. If it is high risk, the command is copied to the clipboard so the user can review before executing manually.
- **Cancel**: Reject the suggestion and continue working or ask the assistant for a new command.

Additional interactions such as explaining or revising a command are handled conversationally. Users can simply ask follow-up questions in natural language (for example, “Explain this”, “Can you modify it?”), and RustyTerm will respond accordingly. When a user initiates a new interaction, previous suggestions are treated as dismissed automatically.

### iii). Contextual Awareness

Rusty-Term doesn’t treat each query in isolation. It optionally reads context like:

- **Current working directory**
- **Environment variables**
- **Last executed commands, with outputs**

This allows AI suggestions to account for context such as project structure, file paths, or previous user actions, making recommendations more accurate and useful.

### iv). Security and Trust Layer

Since AI-generated commands can pose a safety risk if executed blindly, Rusty-Term includes a security analysis module. This performs static analysis on suggested commands to:

- **Detect potentially harmful operations** (such as `rm -rf /`)
- **Trigger warnings for risky actions**
- **Use a built-in allowlist of permitted operations** to avoid flagging known safe commands

This ensures users maintain control and are informed before executing impactful operations.

### v). Session and Context Management

Rusty-Term supports multiple AI sessions, letting users maintain ongoing contexts across different tasks (for example debugging versus file management). Session-based context enhances conversational continuity, allowing the assistant to refine suggestions over time.

---

## Reproducibility Guide

This section provides step-by-step instructions to set up the runtime environment and build the project.

### System Requirements

| Requirement | Details |
|-------------|---------|
| Operating System | Unix-like systems only (macOS, Linux). **Windows is NOT supported.** |
| Rust Version | 1.90 or later |
| Shell | zsh (tested and recommended), bash (should work but untested) |
| OpenAI API Key | Required for AI assistant functionality |

### Step 1: Install Rust Toolchain

If you don't have Rust installed, install it via rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

After installation, ensure your Rust version is 1.90 or later:

```bash
rustc --version
# Expected output: rustc 1.90.0 or higher
```

### Step 2: Clone the Repository

```bash
git clone https://github.com/user/rtdbg.git
cd rtdbg
```

### Step 3: Set Up OpenAI API Key

RustyTerm requires an OpenAI API key to enable AI assistant functionality. Set the environment variable:

```bash
export OPENAI_API_KEY="your-api-key-here"
```

If you want to make this persistent, add the export command to your shell configuration file (`~/.zshrc` for zsh or `~/.bashrc` for bash).

### Step 4: Build and Run

Build and run the application in release mode for optimal performance:

```bash
cargo run --release
```

### Troubleshooting

| Issue | Solution |
|-------|----------|
| `OPENAI_API_KEY not set` error | Ensure the environment variable is exported in your current shell session |
| Build fails with Rust version error | Update Rust: `rustup update` |
| Terminal display issues | Ensure your terminal supports 256 colors and has sufficient size (minimum 80x24) |

---

## User's Guide

This section explains how to use each of the main features in RustyTerm.

### Interface Overview

RustyTerm provides a split-screen TUI (Text User Interface) with two panes:

![RustyTerm Interface Assistant Active](assets/ui_assistant_activated.png)


- **Terminal Panel (Left)**: A fully functional shell session where you can execute commands directly
- **Assistant Panel (Right)**: AI-powered chat interface for natural language queries and command suggestions

The border of active pane is highlighted.

### Keyboard Shortcuts

RustyTerm uses a command mode system (similar to tmux or vim) for navigation and control.

#### Command Mode

Press `Ctrl+B` to enter command mode. While in command mode, the available keyboard shortcuts are displayed.

![Command Mode](assets/command_mode.png)


#### Normal Mode (Terminal Panel)

When the Terminal panel is active, all keyboard input except for `Ctrl + B` is sent directly to the shell, just like a regular terminal.

To send `Ctrl + B` to shell, use `Ctrl + B` `Ctrl + B`.

#### Normal Mode (Assistant Panel)

When the Assistant panel is active, the behavior is mostly like interacting with an input box, with a few exceptions:

| Key | Action |
|-----|--------|
| `Enter` | Send query to the AI |
| `Ctrl + O` | Insert newline (for multi-line input) |
| `Tab` | Switch to next AI session |
| `Ctrl + Y` | Execute (or copy if denied by ) the suggested command |
| `Ctrl + N` | Reject command suggestions |
| `Ctrl + A` | Cycle to next command suggestion (if there are more than one suggestions) |

#### Scrolling

Both panels support scrollback.

| Key | Action |
|-----|--------|
| `Shift + Up/Down` | Scroll one line |
| `Shift + PageUp/PageDown` | Scroll 10 lines |
| `Shift + End` | Scroll to bottom |
| `Esc` | Exit scroll mode (Assistant only) |

#### Visual Mode

Enter visual mode by pressing `Ctrl + B` then `V`. Visual mode allows cursor-based navigation and text selection (vim-style).

| Key | Action |
|-----|--------|
| `h/j/k/l` or Arrow keys | Move cursor |
| `Space` | Cycle selection mode: None → Line → Block |
| `y` | Copy selected text |
| `Shift + Up/Down` | Scroll without moving cursor |
| `PageUp/PageDown` | Scroll 10 lines |
| `1-9` | Repeat count prefix (e.g., `5j` moves down 5 lines) |
| `Esc` | Clear selection, or exit visual mode if no selection |

![Visual Mode](assets/visual_select.png)

### Using the AI Assistant

#### Step 1: Ask a Question

Switch to the Assistant panel. Type your question in natural language. For example:

- "list all rust files in this directory"
- "find files larger than 10MB"
- "show git commit history"
- "check disk usage"

Press `Enter` to send your query.

#### Step 3: Review the AI Response

The AI will respond with:
- A natural language response
- Or, a **Command Card** containing the suggested shell command if requested to generate a command.

The Command Card displays:
- The suggested command
- A brief explanation
- A Security Verdict
- Action buttons

#### Step 4: Confirm or Reject

- Press `Ctrl+Y` to accept the command. Low-risk commands will be injected directly into the terminal. High-risk commands will be copied to your clipboard.
- Press `Ctrl+N` to reject the suggestion.
- To request revisions, explanations, or other suggestions, simply type your follow-up. This will automatically reject any pending commands.

![Command Suggestion](assets/command_suggestion.png)

### Session Management

RustyTerm supports multiple AI conversation sessions, allowing you to maintain separate contexts for different tasks.

- **Create a new session**: In command mode (`Ctrl+B`), create a new session to start a fresh conversation.

- **Switching between sessions**: Press `Tab` to switch to the next session. Session tabs are displayed at the top of the Assistant panel.

- **Close a session**: Press `W` in command mode to close the current session. If it's the last session, it will be cleared instead of closed.

### Mouse Support

RustyTerm supports these mouse operations:

- **Click** to focus a pane
- **Click** on session tabs to create, close and switch sessions.
- **Drag the separator** between panes to resize them
- **Scroll** to navigate through terminal output or chat history
- **Double-click** to select a word
- **Triple-click** to select a line

Mouse events are forwarded to mouse-supported programs (e.g., vim) when the terminal panel is active.

---

## **Contributions by each team member**

- **Weijie (Shell & Context Subsystems)**
  - Built the PTY shell subsystem for executing user commands and capturing terminal output.
  - Implemented the context subsystem for collecting working directory, environment variables, and recent command output.
  - Connected shell and context data to the UI subsystem for real-time updates.
  - Handled concurrency and synchronization between shell I/O and UI rendering.

- **Irys (AI & Security Subsystems)**
  - Implemented the AI subsystem for sending queries and context to the cloud AI service and receiving suggestions.
  - Developed the command suggestion pipeline and integrated AI responses into the UI.
  - Built the security subsystem to analyze AI-generated commands and classify them as safe, warn, or block.
  - Ensured potentially harmful commands never reach the shell subsystem.

- **Yushun (UI & Application Framework)**
  - Implemented the overall TUI structure, including the split-screen layout (shell pane + AI pane).
  - Developed the main event loop and input handling for interactive navigation.
  - Integrated UI updates for shell output, AI responses, and command cards.
  - Ensured smooth rendering and responsiveness across the application.

## **Lessons learned and concluding remarks**

### **Lessons Learned**

This project taught our team several technical and collaborative lessons.

First, building a text based interface that feels modern and responsive in Rust was more challenging than expected. Terminal UI libraries offer a lot of flexibility, but they require careful handling of event loops, rendering updates, and asynchronous tasks. We learned how to structure UI code cleanly and how to avoid blocking operations.

Second, integrating AI into a command line environment raised interesting design questions. For example, we had to decide how natural the assistant’s responses should be, how much context it should remember, and how conservative it should be when suggesting commands that run on a real system. Building a safe and friendly AI assistant required balancing accuracy, user expectations, and system integrity.

Third, working with subprocesses taught us more about shell behavior. Running commands, capturing output, detecting errors, and preventing destructive actions required studying how different shells behave and how Rust interacts with them at the process level.

Fourth, collaboration across the team improved our communication and version control skills. We practiced creating issues, reviewing pull requests, and coordinating changes so that our work merged smoothly.

### **Concluding Remarks**

Rusty Term demonstrates that AI can enhance traditional developer tools without replacing them. The project successfully blends the strengths of a classic terminal with the capabilities of modern language models. The result is a tool that makes command line usage more accessible, especially for users who do not want to memorize dozens of flags or construct long pipelines by hand.

The project also shows the value of Rust as a systems language for building safe and efficient tools. Its strong type system, async capabilities, and ecosystem of libraries helped us create a responsive and reliable application.

Looking ahead, Rusty Term could be expanded with features such as improved command history learning, offline AI models, or integrations with project specific tools like Git or Docker. The current version provides a strong foundation for future development and exploration.

Overall, the project was rewarding from both a technical and educational standpoint. It showcased how AI and systems programming can complement each other to create novel and useful tools for developers.
