//! Application state management.
//!
//! This module defines the main App struct that holds the global state
//! including active pane, shell manager, AI sessions, and context manager.
//! It provides methods for pane switching and state initialization.


use crate::event::{AppEvent, init_app_eventsource, init_user_event};
use crate::event::{assistant as assistant_event, terminal as terminal_event, UserEvent};
use crate::ai::session::AiSessionManager;
use crate::context::ContextManager;
use crate::shell::ShellManager;
use crate::ui::assistant::TuiAssistant;
use crate::ui::terminal::TuiTerminal;
use crate::ui::layout::{AppLayout, LayoutBuilder};
use crate::security::{evaluate, ExecutionDecision, gate_command};


use anyhow::{Context, Result};
use ratatui::DefaultTerminal;
use tokio::sync::mpsc::{Receiver, UnboundedReceiver};

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Terminal,
    Assistant,
}

pub struct App {
    // backend
    shell_manager: ShellManager,
    ai_sessions: AiSessionManager,
    #[allow(unused, reason = "Will be used later")]
    context_manager: ContextManager,

    // frontend widgets
    // they are public to ui module
    pub(in super) tui_terminal: TuiTerminal,  // Terminal widget
    pub(in super) tui_assistant: TuiAssistant,  // Assistant widget

    // App State
    active_pane: ActivePane,  // Which pane is active? (Terminal/Assistant)

    exit: bool,  // Should the app exit?
    command_mode: bool,  // Is the app in the command mode?
    force_redraw_flag: bool,  // Should force a full screen clear and redraw?

    // Layout builder - holds user preferences/constraints for layout
    layout_builder: LayoutBuilder,

    // Current layout - computed from layout_builder and terminal size
    layout: AppLayout,

    // events sources
    user_events: Receiver<std::io::Result<UserEvent>>,  // User input
    app_events: UnboundedReceiver<AppEvent>,  // App Events
}

impl App {
    pub fn new() -> Result<Self> {
        let (event_sink, app_events) = init_app_eventsource();

        // Start with reasonable default size (will be resized on first draw)
        let cols = 80;
        let rows = 24;

        let (shell, pty_rx) = ShellManager::new(event_sink.clone(), cols, rows)?;

        // Create layout builder with default preferences
        let layout_builder = LayoutBuilder::new();

        // Build initial layout from builder
        let initial_area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: cols,
            height: rows,
        };
        let initial_layout = layout_builder.build(initial_area);

        Ok(Self {
            shell_manager: shell,
            // AiSessionManager now owns its own stream channel internally
            ai_sessions: AiSessionManager::new(event_sink.clone(), "gpt-4o-mini")?,
            tui_terminal: TuiTerminal::new(pty_rx, event_sink.clone()),
            tui_assistant: TuiAssistant::new(),
            active_pane: ActivePane::Terminal,
            context_manager: ContextManager::new(),
            exit: false,
            command_mode: false,
            force_redraw_flag: false,
            layout_builder,
            layout: initial_layout,
            user_events: init_user_event(),
            app_events,
        })
    }

    pub fn get_active_pane(&self) -> ActivePane {
        self.active_pane
    }

    pub fn switch_pane(&mut self, pane: ActivePane) {
        self.active_pane = pane;
    }
    pub fn toggle_pane(&mut self) {
        let next = match self.active_pane {
            ActivePane::Terminal => ActivePane::Assistant,
            ActivePane::Assistant => ActivePane::Terminal,
        };
        self.switch_pane(next);
    }

    pub fn get_command_mode(&self) -> bool {
        self.command_mode
    }

    /// Single execution entrypoint that enforces security verdict gating.
    ///
    /// This method is the ONLY way commands should be executed from AI suggestions.
    /// It evaluates the command, checks the verdict, and decides whether to:
    /// - Execute immediately (Allow)
    /// - Require user confirmation (RequireConfirmation)
    /// - Deny execution (Deny)
    ///
    /// # Arguments
    /// * `cmd` - The command string to execute
    ///
    /// # Returns
    /// * `Ok(())` if the command was handled appropriately
    /// * `Err(_)` if execution failed
    ///
    /// # Behavior by Verdict
    /// - `Allow`: Executes immediately via `execute_visible()`
    /// - `RequireConfirmation`: Returns Ok without executing (UI handles confirmation)
    /// - `Deny`: Returns Ok without executing and surfaces error to UI
    ///
    /// # Examples
    /// ```no_run
    /// # use rusty_term::app::App;
    /// # fn example(app: &mut App) -> anyhow::Result<()> {
    /// // Safe command - executes immediately
    /// app.try_execute_suggested("ls -la")?;
    ///
    /// // Dangerous command - denied, error shown to user
    /// app.try_execute_suggested("rm -rf /")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn try_execute_suggested(&mut self, cmd: &str) -> Result<()> {
        // Evaluate the command to get its security verdict
        let verdict = evaluate(cmd);
        
        // Gate the command based on its verdict
        let decision = gate_command(cmd, verdict);
        
        match decision {
            ExecutionDecision::Execute => {
                // Allow verdict: execute immediately
                self.shell_manager
                    .execute_visible(cmd)
                    .context("Failed to execute allowed command")?;
            }
            ExecutionDecision::RequireConfirmation { reason } => {
                // RequireConfirmation verdict: do not execute yet
                // The UI already shows the command card with confirmation prompt
                // This is handled by the confirm_command flow in the event handler
                // Log the reason for debugging
                let _ = reason; // Suppress unused warning
                // No execution happens here - user must confirm first
            }
            ExecutionDecision::Deny { reason } => {
                // Deny verdict: do not execute, surface error to UI
                self.tui_terminal.show_error(&format!("Command denied: {}", reason));
            }
        }
        
        Ok(())
    }

    pub fn set_command_mode(&mut self, flag: bool) {
        self.command_mode = flag;
    }

    pub fn toggle_command_mode(&mut self) {
        self.command_mode = !self.command_mode;
    }

    /// Check if the active pane is in visual mode.
    /// Visual mode state is owned by each component, App just queries it.
    pub fn is_visual_mode(&self) -> bool {
        match self.active_pane {
            ActivePane::Terminal => self.tui_terminal.is_visual_mode(),
            ActivePane::Assistant => self.tui_assistant.is_visual_mode(),
        }
    }

    /// Enter visual mode for the active pane.
    pub fn enter_visual_mode(&mut self) {
        match self.active_pane {
            ActivePane::Terminal => self.tui_terminal.enter_visual_mode(),
            ActivePane::Assistant => self.tui_assistant.enter_visual_mode(),
        }
    }

    /// Get current layout
    pub fn layout(&self) -> &AppLayout {
        &self.layout
    }

    /// Get current layout builder (configuration) - read only
    pub fn layout_builder(&self) -> &LayoutBuilder {
        &self.layout_builder
    }

    /// Set layout builder and trigger layout rebuild
    ///
    /// This replaces the entire layout configuration and recalculates the layout.
    /// Use this when loading saved preferences or making multiple configuration changes.
    ///
    /// # Arguments
    /// * `builder` - The new layout builder configuration
    pub fn set_layout_builder(&mut self, builder: LayoutBuilder) {
        // Always rebuild layout when builder changes
        if self.layout_builder != builder {
            self.layout_builder = builder;
            self.rebuild_layout(self.layout.full_area);
        }
    }

    /// Get current split ratio (percentage for terminal pane)
    pub fn split_ratio(&self) -> u16 {
        self.layout_builder.split_ratio()
    }

    /// Set split ratio (user preference) and trigger layout rebuild
    ///
    /// This ratio persists across window resizes. For example, if user drags
    /// the separator to 70/30, this ratio will be maintained even when the
    /// window is resized.
    ///
    /// # Arguments
    /// * `ratio` - Percentage (0-100) for terminal pane width
    pub fn set_split_ratio(&mut self, ratio: u16) {
        let old_ratio = self.layout_builder.split_ratio();
        self.layout_builder = self.layout_builder.with_split_ratio(ratio);

        // Only rebuild if ratio actually changed
        if self.layout_builder.split_ratio() != old_ratio {
            self.rebuild_layout(self.layout.full_area);
        }
    }



    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            if self.exit {
                break Ok(());
            }
            tokio::select! {
                res = self.user_events.recv() => {
                    let usr_evt = res.with_context(|| anyhow::anyhow!("User event stream is ended."))?;
                    self.handle_user_event(usr_evt?)?;
                }
                res = self.app_events.recv() => {
                    let app_evt = res.with_context(|| anyhow::anyhow!("App event stream is ended"))?;
                    self.handle_app_event(app_evt)?;
                }
                // AiSessionManager receives stream data, stores it, and returns UI updates
                update = self.ai_sessions.recv_ai_stream() => {
                    if let Some(update) = update {
                        // Forward UI update to TuiAssistant for display
                        self.tui_assistant.handle_ai_update(update);
                    }
                }
                _ = self.tui_terminal.recv_pty_output() => {
                    // PTY output is handled internally by TuiTerminal
                }
            }
            // Check if force redraw is needed (e.g., after stderr pollution)
            if self.force_redraw_flag {
                self.force_redraw_flag = false;
                self.force_redraw(terminal)?;
            } else {
                self.draw(terminal)?;
            }
        }
    }

    pub fn draw(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // Render the UI
        terminal.draw(|frame| {
            let area = frame.area();

            // Rebuild layout if terminal size changed
            if self.layout.full_area != area {
                self.rebuild_layout(area);
            }

            // Render using Widget trait
            use ratatui::widgets::Widget;
            (&*self).render(area, frame.buffer_mut());
        })?;

        // Set cursor based on current layout
        self.update_cursor_position(terminal)?;

        Ok(())
    }

    /// Force a full screen clear and redraw.
    /// This is useful when stderr output has polluted the screen.
    pub fn force_redraw(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // eprintln!("force_redraw");
        terminal.clear()?;
        self.draw(terminal)?;
        Ok(())
    }

    /// Rebuild layout from current builder configuration and area
    ///
    /// This method also handles terminal resize when the terminal pane size changes.
    fn rebuild_layout(&mut self, area: ratatui::layout::Rect) {
        let old_layout = self.layout;
        self.layout = self.layout_builder.build(area);

        // Resize terminal and PTY if terminal pane size changed
        let new_term_area = self.layout.terminal_inner;
        let old_term_area = old_layout.terminal_inner;
        if new_term_area.width != old_term_area.width || new_term_area.height != old_term_area.height {
            self.tui_terminal.resize(new_term_area.width, new_term_area.height);
            if let Err(e) = self.shell_manager.resize(new_term_area.width, new_term_area.height) {
                eprintln!("Failed to resize PTY: {}", e);
            }
        }
    }

    fn update_cursor_position(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // In visual mode or command mode, hide the hardware cursor
        // (visual mode cursor is rendered as a highlighted cell)
        if self.get_command_mode() || self.is_visual_mode() {
            terminal.hide_cursor()?;
            return Ok(());
        }

        // Directly match on app state and use layout areas
        match self.get_active_pane() {
            // Terminal pane active: show cursor at terminal position
            ActivePane::Terminal => {
                let term_area = self.layout.terminal_inner;
                let (cursor_row, cursor_col) = self.tui_terminal.cursor_position();
                let cursor_x = term_area.x + cursor_col;
                let cursor_y = term_area.y + cursor_row;

                terminal.show_cursor()?;
                terminal.set_cursor_position((cursor_x, cursor_y))?;
            }

            // Assistant pane active: show cursor at assistant input position
            ActivePane::Assistant => {
                let ai_area = self.layout.assistant_inner;

                // Calculate the dynamic input box height
                let input_box_height = self.tui_assistant.calculate_input_box_height(ai_area.height, ai_area.width);

                // Calculate the input box area
                let input_box_y = ai_area.y + ai_area.height.saturating_sub(input_box_height);
                let input_box_x = ai_area.x;

                // The input box has a top border, so the inner area is:
                let inner_y = input_box_y + 1;

                // Get cursor position from assistant
                if let Some((rel_x, rel_y)) = self.tui_assistant.get_cursor_position() {
                    terminal.show_cursor()?;
                    terminal.set_cursor_position((input_box_x + rel_x, inner_y + rel_y))?;
                } else {
                    terminal.hide_cursor()?;
                }
            }
        }

        Ok(())
    }
}


impl App {

    fn handle_user_event(&mut self, event: UserEvent) -> Result<()>  {
        if self.command_mode {
            self.handle_command_mode_events(event)?;
            return Ok(());
        }

        // Handle visual mode events (delegated to component)
        if self.is_visual_mode() {
            if let UserEvent::Key(key) = event {
                let result = match self.active_pane {
                    ActivePane::Terminal => self.tui_terminal.handle_visual_key(key),
                    ActivePane::Assistant => self.tui_assistant.handle_visual_key(key),
                };
                match result {
                    crate::ui::visual::KeyHandleResult::RequestCommandMode => {
                        self.set_command_mode(true);
                    }
                    _ => {}
                }
            }
            return Ok(());
        }

        match event {
            UserEvent::Key(key_evt) if matches!(key_evt.kind, KeyEventKind::Press) => {
                // Ctrl + B => Command Mode
                if key_evt.modifiers.contains(KeyModifiers::CONTROL) && matches!(key_evt.code, KeyCode::Char('b') | KeyCode::Char('B')) {
                    self.set_command_mode(true);
                    return Ok(());
                }

                // Pane-specific event handling (delegated to event module)
                match self.active_pane {
                    ActivePane::Terminal => {
                        terminal_event::handle_key_event(&mut self.tui_terminal, &mut self.shell_manager, key_evt)?;
                    }
                    ActivePane::Assistant => {
                        // Get current context snapshot for AI requests
                        assistant_event::handle_key_event(
                            &mut self.tui_assistant,
                            &mut self.ai_sessions,
                            &self.context_manager,
                            key_evt,
                        )?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_command_mode_events(&mut self, event: UserEvent) -> Result<()> {
        assert!(self.command_mode);

        // Common commands (available in both panes)
        match &event {
            // n => toggle pane (switch between Terminal and Assistant)
            UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('n') | KeyCode::Char('N')) => {
                self.toggle_pane();
                self.set_command_mode(false);
                return Ok(());
            }

            // q => exit application
            UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('q') | KeyCode::Char('Q')) => {
                self.exit = true;
                self.set_command_mode(false);
                return Ok(());
            }

            // l => force redraw (refresh all, clear stderr pollution)
            UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('l') | KeyCode::Char('L')) => {
                self.force_redraw_flag = true;
                self.set_command_mode(false);
                return Ok(());
            }

            // v => enter visual mode
            UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('v') | KeyCode::Char('V')) => {
                self.set_command_mode(false);
                self.enter_visual_mode();
                return Ok(());
            }

            _ => {}
        }

        // Pane-specific commands
        match self.active_pane {
            ActivePane::Terminal => {
                crate::event::terminal::handle_command_mode(
                    &mut self.tui_terminal,
                    &mut self.shell_manager,
                    event,
                )?;
            }
            ActivePane::Assistant => {
                crate::event::assistant::handle_command_mode(
                    &mut self.tui_assistant,
                    &mut self.ai_sessions,
                    event,
                )?;
            }
        }

        self.set_command_mode(false);
        Ok(())
    }

}

impl App {
    fn handle_app_event(&mut self, event: AppEvent) -> Result<()> {
        match event {
            AppEvent::PtyWrite(s) => {
                self.shell_manager.handle_user_input(&s)?;
            }

            // AI Events
            AppEvent::ExecuteAiCommand { session_id: _, command } => {
                // Execute through the security gate (single entrypoint)
                self.try_execute_suggested(&command)?;
            }

            // Shell Events
            AppEvent::ShellError { message } => {
                // Display error in terminal pane
                self.tui_terminal.show_error(&message);

                // If shell exited, mark app for exit
                if message.contains("exited") {
                    self.exit = true;
                }
            }

            AppEvent::ShellCommandCompleted { command, exit_code } => {
                self.context_manager.history.push(command);
                let _ = exit_code; // Suppress unused warnings for now
            }

            AppEvent::ShellOutput { data } => {
                self.context_manager.push_output(data);
            }
        }
        Ok(())
    }
}
