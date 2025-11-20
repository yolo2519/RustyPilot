//! Application state management.
//! 
//! This module defines the main App struct that holds the global state
//! including active pane, shell manager, AI sessions, and context manager.
//! It provides methods for pane switching and state initialization.

use crate::ai::session::AiSessionManager;
use crate::context::ContextManager;
use crate::shell::ShellManager;

use anyhow::Result;

pub enum ActivePane {
    Shell,
    Ai,
}

pub struct App {
    pub active_pane: ActivePane,
    pub shell_manager: ShellManager,
    pub ai_sessions: AiSessionManager,
    pub context_manager: ContextManager,
}

impl App {
    pub fn new() -> Result<Self> {
        Ok(Self {
            active_pane: ActivePane::Shell,
            shell_manager: ShellManager::new()?,
            ai_sessions: AiSessionManager::new(),
            context_manager: ContextManager::new(),
        })
    }

    pub fn switch_pane(&mut self) {
        self.active_pane = match self.active_pane {
            ActivePane::Shell => ActivePane::Ai,
            ActivePane::Ai => ActivePane::Shell,
        };
    }
}
