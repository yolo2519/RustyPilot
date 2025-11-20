//! Terminal initialization and restoration utilities.
//! 
//! This module provides functions to initialize the terminal in raw mode,
//! set up the alternate screen for TUI rendering, and restore the terminal
//! to its normal state upon exit.

use std::io::{stdout, Stdout};

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

pub type TuiTerminal = Terminal<CrosstermBackend<Stdout>>;

pub fn init_terminal() -> Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, LeaveAlternateScreen)?;
    Ok(())
}
