//! Main entry point for RustyTerm application.
//! 
//! This file initializes the TUI terminal, creates the application state,
//! runs the main event loop, and handles graceful terminal restoration on exit.

mod ui;
mod shell;
mod ai;
mod context;
mod security;
mod shell;
mod ui;
mod utils;

use anyhow::Result;
use ui::app::App;
use ui::layout::calculate_shell_size;
use ui::terminal;

#[tokio::main]
async fn main() -> Result<()> {
    // initialize logging
    utils::logger::init_logging();

    // initialize terminal TUI
    let mut terminal = terminal::init_terminal()?;

    // initialize app state
    let size = terminal.size()?;
    let (shell_cols, shell_rows) = calculate_shell_size(size.width, size.height);

    let mut app = App::new(shell_cols, shell_rows)?;

    // main event loop (sync or async, here we leave the async interface)
    ui::event_loop::run(&mut terminal, &mut app).await?;

    // restore terminal
    terminal::restore_terminal()?;
    Ok(())
}
