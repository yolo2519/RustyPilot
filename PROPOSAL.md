# RustyTerm: Rust-Based AI Copilot for the Command Line

## **Team Information**


| Member      | Student Number | Email                        |
|-------------|----------------|------------------------------|
| Weijie Zhu  | 1009310906     | weijie.zhu@mail.utoronto.ca  |
| Irys Zhang  | 1012794424     | irys.zhang@mail.utoronto.ca  |
| Yushun Tang | 1011561962     | yushun.tang@mail.utoronto.ca |


---

## **Introduction**

In the modern developer workflow, the command line interface (CLI) remains one of the most powerful tools for interacting with a system. Although the command line interface is widely used, many developers, including those with significant experience, often find it challenging to recall the exact syntax, flags, or command combinations. Simple tasks such as searching files, piping output, or setting permissions often require looking up documentation or consulting online AI tools like ChatGPT or Claude. This constant context-switching between the terminal and browser interrupts workflow and reduces productivity.

This proposal presents a Rust-based AI assistant integrated directly into the command line environment, inspired by tools like GitHub Copilot but designed specifically for shell interactions. The goal is to provide a smooth, intelligent, and secure experience where users can query, generate, and execute shell commands seamlessly without leaving the terminal. The assistant will be implemented as a Text-based User Interface (TUI) application, combining the robustness and safety of Rust with modern AI integration and system-level concurrency.

## **Motivation**

The primary motivation behind this project is to simplify and streamline the user experience in shell environments. Developers and system administrators often encounter repetitive challenges when they know the task they want to perform but cannot remember the precise command syntax to achieve it. Searching for solutions in web browsers or AI chats forces a disruptive switch from the terminal to another application, breaking cognitive flow.

### **1\. Addressing a Real-World Pain Point**

When using Unix-like shells such as Zsh or Bash, users often need to perform operations such as text processing with *awk*, searching with *grep*, or network configuration with *curl* \- commands that can be complex and easy to forget. This proposal seeks to solve that inefficiency by bringing AI-powered command assistance directly into the shell interface. Instead of copying and pasting between ChatGPT and the terminal, users will receive instant, context-aware command suggestions within the same environment.

### **2\. Integration vs. Fragmentation**

Current AI tools such as ChatGPT, Claude, or AIChat provide excellent conversational capabilities but often require users to switch applications or run separate shell commands to interact with the assistant. For instance, AIChat operates as an inline command (*aichat 'query'*), which leaves chat history within the command buffer and disrupts the terminal’s cleanliness. In contrast, our proposed system draws inspiration from Cursor’s sidebar interface and the layout flexibility of tmux, integrating an intelligent, interactive side panel directly within the shell environment. This approach maintains a clear workspace, ensures smooth interaction, and enhances usability by allowing users to focus on their workflow without unnecessary context switching.

### **3\. Relevance to the Rust Ecosystem**

Rust is an ideal language for developing such a tool due to its performance, safety, and concurrency capabilities. The Rust ecosystem offers multiple mature libraries for TUI development, such as *tui-rs* and *ratatui*, as well as async runtimes like *tokio* for managing concurrent AI requests. Developing this assistant takes full advantage of Rust’s ecosystem strengths and addresses a current gap in the field, as there are very few Rust-based AI terminal integrations that offer interactive, multi-session functionality.

### **4\. Broader Impact and Innovation**

Beyond developer convenience, this project contributes to the ongoing discussion of human-AI interaction in low-UI environments. By embedding AI into the shell, we move toward more intelligent and context-aware interfaces that effectively bridge human intent with system execution. The assistant represents a step toward a natural-language-operable terminal, an innovation that could influence the design of future developer tools and AI-powered systems.

## **Objective and key features**

The primary objective of this project is to design and implement a Rust-based AI assistant for shell environments that provides intelligent, secure, and context-aware command recommendations. The proposed system will operate as a split-screen TUI application: the left pane will contain a functional shell environment (e.g.,Zsh), and the right pane will host the AI assistant interface.

### **1\. Core Objectives**

* Enhance productivity by reducing time spent switching between applications for command lookup.
* Provide AI-generated command suggestions that are contextually relevant to the user’s current working directory, environment variables, and command history.
* Ensure transparency and control by allowing users to review and understand AI-suggested commands before execution.
* Demonstrate the potential of Rust for building robust, concurrent, and user-friendly terminal applications.

### **2\. Key Features**

#### **2.1. TUI Split Interface**

* The interface will feature a dual-pane layout, similar to modern IDEs like VSCode or Cursor.
* The left pane will host the active shell session (Zsh or Bash).
* The right pane will serve as the AI sidebar, where users can type queries, receive suggestions, and interact with the assistant.
* Navigation between panes will be keyboard-driven for efficiency.

#### **2.2. AI Command Suggestions**

The assistant will process natural-language input (e.g., “find all Python files larger than 1MB”) and return an appropriate shell command. The user can then choose among several actions:

* **Explain:** Ask the AI to explain what the suggested command does and its potential side effects.
* **Accept:** Paste the command into the shell (configurable for automatic or manual execution).
* **Revise:** Request an improved or modified version of the command.
* **Decline:** Dismiss the suggestion.

#### **2.3. Context Awareness**

The assistant will, optionally, read contextual data such as:

* The current working directory (*cwd*)
* Environment variables
* Output of the previous command
  This feature enables the AI to tailor suggestions accurately (e.g., knowing when the user is in a Git repository or a project directory).

#### **2.4. Security and Trust Layer**

Given that AI-generated commands may involve sensitive operations, a security module will be developed.

* Commands will be analyzed before execution to detect potentially harmful operations (e.g., *rm \-rf* /).
* A user-defined allowlist will specify which operations the AI is permitted to perform automatically.
* Potentially dangerous commands will trigger warnings or require confirmation.

#### **2.5. Session and Context Management**

* Multiple AI sessions will be supported, allowing users to maintain different tasks or topics (e.g., debugging vs. file management).
* Session context will persist temporarily, enabling conversational continuity and iterative command refinement.

#### **2.6. Extensibility**

While the core product focuses on shell command assistance, the architecture will allow for future extensions such as:

* Integration with code editors (e.g., Neovim)
* Command analytics for frequently used operations
* Multi-language shell support (Zsh, Bash, Fish)
* Cloud-based AI model connections for personalized training.

### **3\. Innovation and Differentiation**

Unlike existing solutions that either occupy the terminal space or clutter command history, this project introduces a cleanly integrated visual and conversational layer inside the shell. The assistant will not merely act as a chatbot; it will augment the command-line experience, enabling direct interaction between human intent and system command execution. This represents a meaningful innovation in the space of developer productivity tools.

## **Tentative plan**

The development process will be structured in three major phases \- Design & Prototyping, Implementation & Testing, and Evaluation & Refinement. Each phase will last approximately 3-4 weeks, aligning with a standard semester timeline.

### **Phase 1: Design and Prototyping (Weeks 1–4)**

**Goals:**

* Finalize system architecture and user interface layout.
* Research existing Rust TUI frameworks (*tui-rs, ratatui, crossterm*) and select the most suitable.
* Design the AI integration layer by connecting directly to an external API service, such as the OpenAI API, for generating command suggestions and explanations.
* Build wireframes of the split-screen interface and interaction flow.

**Deliverables:**

* Preliminary design document and mockups.
* CLI-based prototype demonstrating text input and output in a dual-pane layout.

### **Phase 2: Implementation and Testing (Weeks 5–9)**

**Goals:**

* Implement TUI structure with concurrency support using *tokio*.
* Integrate shell (Zsh/Bash) subprocess management for command execution.
* Implement the AI backend for natural-language interpretation and command generation using the *`async-openai`* crate to handle asynchronous API communication efficiently.
* Develop a security module (command analyzer and allowlist system).
* Add explain/accept/revise/decline functionality with keyboard shortcuts.

**Deliverables:**

* Fully functional TUI prototype with interactive shell and AI sidebar.
* Security checks in place for unsafe command detection.
* Internal test cases for user interactions and command execution.

### **Phase 3: Evaluation, Optimization, and Refinement (Weeks 10–13)**

**Goals:**

* Conduct usability testing with peers and gather feedback on command accuracy, safety, and UI clarity.
* Optimize performance and memory usage.
* Implement optional context awareness features (e.g., reading last command output).
* Prepare final presentation and documentation.

**Deliverables:**

* Completed application with stable performance.
* Final report documenting design choices, limitations, and future work.

### **Division of Work and Roles**

Each team member will specialize in one domain, ensuring both focus and collaboration:

| Category | Task Description | Yushun Tang | Weijie Zhu | Irys Zhang |
| :---- | :---- | :---- | :---- | :---- |
| UI/UX Development | TUI layout design using *tui-rs* / *ratatui* | ✓ | ✓ |  |
|  | Keyboard navigation and shortcut implementation | ✓ | ✓ |  |
|  | Input handling and user feedback collection  | ✓ |  | ✓ |
|  | Color theme, text hierarchy, and layout responsiveness | ✓ |  | ✓ |
| AI Integration | API setup and connection management (e.g., OpenAI API) |  | ✓ |  |
|  | Prompt design and structured response parsing |  | ✓ | ✓ |
|  | Natural language understanding and command mapping |  | ✓ |  |
|  | Implement “Explain / Accept / Revise / Decline” actions |  | ✓ | ✓ |
| System Development | Shell integration and subprocess handling |  |  | ✓ |
|  | Concurrency management using *tokio* |  |  | ✓ |
|  | Context capturing (CWD, environment variables, history) | ✓ |  | ✓ |
|  | Security module (allowlist, command validation) |  | ✓ | ✓ |
| Testing & Documentation | Unit and integration test design | ✓ | ✓ | ✓ |
|  | Usability testing and user feedback analysis | ✓ | ✓ | ✓ |
|  | Technical documentation and user guide writing | ✓ | ✓ | ✓ |
|  | Final presentation and report preparation | ✓ | ✓ | ✓ |

##

### **Development Tools and Libraries**

* **Language:** Rust
* **Libraries:** *tui-rs / ratatui, tokio, serde, reqwest, crossterm*
* **AI Backend:** OpenAI API or equivalent LLM interface
* **Version Control:** GitHub
* **Testing Framework:** *cargo test* with mock input/output validation

  ###

### **Evaluation Metrics**

Project success will be assessed through a combination of practical testing, and basic performance checks. The evaluation will focus on whether the system functions reliably and provides a smooth, helpful user experience.

* **Functionality Accuracy:** Evaluate how often the AI produces reasonable or partially correct command suggestions based on user prompts.
* **System Responsiveness:** Observe whether the TUI and shell interactions remain stable and responsive during normal use.
* **Basic Safety Checks:** Confirm that the program avoids executing clearly unsafe commands and handles unexpected inputs gracefully.

### **Expected Outcomes and Learning Goals**

This project will yield a fully functional AI-powered TUI shell assistant and deepen the team’s understanding of key Rust and systems programming concepts. Expected learning outcomes include:

1. **Advanced Rust Programming:** Mastery of concurrency, error handling, and asynchronous programming.
2. **TUI Design and Usability:** Understanding terminal rendering, event-driven architecture, and input handling.
3. **AI and API Integration:** Practical experience in connecting language models to real-world interfaces.
4. **Security Awareness:** Implementing safe execution layers in system-level tools.
5. **Team Collaboration:** Coordinating development tasks and integrating modular components efficiently.

The resulting product will not only demonstrate technical competence but also serve as a valuable contribution to the open-source Rust ecosystem, potentially inspiring future tools and research into AI-assisted command-line systems.

## **Conclusion**

This proposal outlines a comprehensive plan to design and implement an AI-powered shell assistant in Rust, addressing a common productivity bottleneck among developers: the constant need to recall or look up shell commands. By merging AI assistance directly into the terminal environment, this project aims to create a fluid, context-aware, and secure user experience that enhances both efficiency and learning.

The assistant represents a fusion of practical utility, technical challenge, and innovation, utilizing Rust’s concurrency and safety features to create a robust and interactive TUI. Beyond its immediate functionality, the project embodies a forward-looking vision: bringing AI into everyday developer workflows seamlessly and responsibly.

In achieving this, our team expects not only to produce a valuable tool but also to gain deep insight into system design, AI integration, and the human factors that shape the future of programming interfaces.

