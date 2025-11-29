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
    fn handle_events(&mut self) -> Result<()> {
        if !event::poll(Duration::ZERO)? {
            return Ok(());
        }
        match event::read()? {
            Event::Key(key_evt) if matches!(key_evt.kind, KeyEventKind::Press) => {
                if key_evt.modifiers.contains(KeyModifiers::CONTROL) && matches!(key_evt.code, KeyCode::Char('c')) {
                    self.exit = true;
                }
            }
            _ => {}
        }
        Ok(())
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
