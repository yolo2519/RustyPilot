//! Application state management.
//!
//! This module defines the main App struct that holds the global state
//! including active pane, shell manager, AI sessions, and context manager.
//! It provides methods for pane switching and state initialization.


use crate::event::{AppEvent, init_app_eventsource, init_user_event};
use crate::{ai::session::AiSessionManager, event::UserEvent};
use crate::context::ContextManager;
use crate::shell::ShellManager;
use crate::ui::assistant::TuiAssistant;
use crate::ui::terminal::TuiTerminal;


use anyhow::{Context, Result};
use ratatui::DefaultTerminal;
use tokio::sync::mpsc::Receiver;

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Terminal,
    Assistant,
}

pub struct App {
    // backend
    #[allow(unused, reason = "Will be used later")]
    shell_manager: ShellManager,
    #[allow(unused, reason = "Will be used later")]
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
    app_events: Receiver<AppEvent>,  // App Events
}

impl App {
    pub fn new() -> Result<Self> {
        let (event_sink, app_events) = init_app_eventsource();
        let (shell, pty_rx) = ShellManager::new(event_sink.clone())?;
        Ok(Self {
            shell_manager: shell,
            ai_sessions: AiSessionManager::new(),
            tui_terminal: TuiTerminal::new(pty_rx, event_sink.clone()),
            tui_assistant: TuiAssistant::new(),
            active_pane: ActivePane::Terminal,
            context_manager: ContextManager::new(),
            exit: false,
            command_mode: false,
            user_events: init_user_event(),
            app_events
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
                    // TODO: handle these events
                    let app_evt = res.with_context(|| anyhow::anyhow!("App event stream is ended"))?;
                    self.handle_app_event(app_evt)?;
                }
                // _ = tokio::time::sleep(Duration::from_secs(1)) => {}
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
                // mock event: Ctrl + C => Exit
                if key_evt.modifiers.contains(KeyModifiers::CONTROL) && matches!(key_evt.code, KeyCode::Char('c')) {
                    self.exit = true;
                }
                // mock event: Ctrl + B => Command Mode
                if key_evt.modifiers.contains(KeyModifiers::CONTROL) && matches!(key_evt.code, KeyCode::Char('b')) {
                    self.set_command_mode(true);
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
    fn handle_app_event(&mut self, _event: AppEvent) -> Result<()> {
        // TODO: implement event handler
        Ok(())
    }
}
