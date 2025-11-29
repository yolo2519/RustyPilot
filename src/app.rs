//! Application state management.
//!
//! This module defines the main App struct that holds the global state
//! including active pane, shell manager, AI sessions, and context manager.
//! It provides methods for pane switching and state initialization.

use std::time::Duration;

use crate::ai::session::AiSessionManager;
use crate::context::ContextManager;
use crate::shell::ShellManager;
use crate::ui::assistant::TuiAssistant;
use crate::ui::terminal::TuiTerminal;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;


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

    // frontend widgets
    // they are public to ui module
    pub(in super) tui_terminal: TuiTerminal,
    pub(in super) tui_assistant: TuiAssistant,

    // App State
    active_pane: ActivePane,
    #[allow(unused, reason = "Will be used later")]
    context_manager: ContextManager,
    exit: bool,
    command_mode: bool,
}

impl App {
    pub fn new() -> Result<Self> {
        Ok(Self {
            shell_manager: ShellManager::new()?,
            ai_sessions: AiSessionManager::new(),
            tui_terminal: TuiTerminal::new(),
            tui_assistant: TuiAssistant::new(),
            active_pane: ActivePane::Terminal,
            context_manager: ContextManager::new(),
            exit: false,
            command_mode: false,
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

    fn handle_events(&mut self) -> Result<()> {
        if !event::poll(Duration::ZERO)? {
            return Ok(());
        }

        if self.command_mode {
            self.handle_command_mode_events(event::read()?);
            return Ok(());
        }
        match event::read()? {
            Event::Key(key_evt) if matches!(key_evt.kind, KeyEventKind::Press) => {
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

    fn handle_command_mode_events(&mut self, event: Event) {
        assert!(self.command_mode);
        use Event::*;
        match event {
            // mock event: n => toggle pane
            Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('n')) => {
                self.toggle_pane();
            }
            // mock event: c => exit
            Key(e) if matches!(e.kind, KeyEventKind::Press) && matches!(e.code, KeyCode::Char('c')) => {
                self.exit = true;
            }
            _ => {},
        }
        self.set_command_mode(false);
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        // TODO: main loop of App::run
        while !self.exit {
            self.handle_events()?;
            terminal.draw(|frame| {
                let area = frame.area();
                frame.render_widget(&*self, area);
            })?;
        }
        Ok(())
    }
}
