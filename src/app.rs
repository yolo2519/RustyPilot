//! Application state management.
//!
//! This module defines the main App struct that holds the global state
//! including active pane, shell manager, AI sessions, and context manager.
//! It provides methods for pane switching and state initialization.


use crate::event::{AiStreamData, AppEvent, init_app_eventsource, init_user_event};
use crate::event::{assistant as assistant_event, terminal as terminal_event, UserEvent};
use crate::ai::session::AiSessionManager;
use crate::context::ContextManager;
use crate::shell::ShellManager;
use crate::ui::assistant::TuiAssistant;
use crate::ui::terminal::TuiTerminal;


use anyhow::{Context, Result};
use ratatui::DefaultTerminal;
use tokio::sync::mpsc::{self, Receiver, UnboundedReceiver};

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
        
        // Create dedicated channel for AI streaming (high-frequency data)
        let (ai_stream_tx, ai_stream_rx) = mpsc::channel::<AiStreamData>(256);
        
        Ok(Self {
            shell_manager: shell,
            ai_sessions: AiSessionManager::new(ai_stream_tx, event_sink.clone(), "gpt-4o-mini"),
            tui_terminal: TuiTerminal::new(pty_rx, event_sink.clone()),
            tui_assistant: TuiAssistant::new(ai_stream_rx),
            active_pane: ActivePane::Terminal,
            context_manager: ContextManager::new(),
            exit: false,
            command_mode: false,
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

    pub fn set_command_mode(&mut self, flag: bool) {
        self.command_mode = flag;
    }

    pub fn toggle_command_mode(&mut self) {
        self.command_mode = !self.command_mode;
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
                _ = self.tui_assistant.recv_ai_stream(&mut self.ai_sessions) => {
                    // AI stream data is handled internally by TuiAssistant
                }
                _ = self.tui_terminal.recv_pty_output() => {
                    // PTY output is handled internally by TuiTerminal
                }
            }
            self.draw(terminal)?;
        }
    }

    pub fn draw(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(&*self, area);
        })?;
        Ok(())
    }
}


impl App {

    fn handle_user_event(&mut self, event: UserEvent) -> Result<()>  {
        if self.command_mode {
            self.handle_command_mode_events(event)?;
            return Ok(());
        }
        match event {
            UserEvent::Key(key_evt) if matches!(key_evt.kind, KeyEventKind::Press) => {
                // Ctrl + B => Command Mode
                if key_evt.modifiers.contains(KeyModifiers::CONTROL) && matches!(key_evt.code, KeyCode::Char('b')) {
                    self.set_command_mode(true);
                    return Ok(());
                }

                // Pane-specific event handling (delegated to event module)
                match self.active_pane {
                    ActivePane::Terminal => {
                        terminal_event::handle_key_event(&mut self.tui_terminal, &mut self.shell_manager, key_evt)?;
                    }
                    ActivePane::Assistant => {
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

        match event {
            // mock event: n => toggle pane
            UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('n')) => {
                self.toggle_pane();
            }
            // mock event: c => exit
            UserEvent::Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('c')) => {
                self.exit = true;
            }
            _ => {
                return Ok(());
                // don't allow other ignored events to exit command mode
            },
        }
        self.set_command_mode(false);
        Ok(())
    }
}

impl App {
    fn handle_app_event(&mut self, event: AppEvent) -> Result<()> {
        match event {
            // AI Events
            AppEvent::AiCommandSuggestion {
                session_id,
                command,
                explanation,
            } => {
                if session_id == self.tui_assistant.active_session_id() {
                    self.tui_assistant.push_command_card(command, explanation);
                }
            }

            AppEvent::ExecuteAiCommand { session_id } => {
                // Retrieve the command from the AI session
                if let Some(suggestion) = self.ai_sessions.get_last_suggestion(session_id) {
                    let command = suggestion.suggested_command.clone();
                    // Inject the command into the shell
                    self.shell_manager.inject_command(&command)?;
                }
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
        }
        Ok(())
    }
}
