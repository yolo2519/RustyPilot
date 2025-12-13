//! Main entry point for RustyTerm application.
//!
//! This file initializes the TUI terminal, creates the application state,
//! runs the main event loop, and handles graceful terminal restoration on exit.

use rusty_term::utils;
use rusty_term::app;

use anyhow::Result;
use app::App;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

use crate::utils::context::Context;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging before anything else
    utils::logger::init_logging();

    let mut terminal = ratatui::init();

    // Enable mouse capture for mouse event handling
    execute!(std::io::stdout(), EnableMouseCapture)?;

    // Context guard ensures cleanup on both normal exit and panic
    let _ctx = Context::with(|| {
        // Disable mouse capture before restoring terminal
        if let Err(e) = execute!(std::io::stdout(), DisableMouseCapture) {
            tracing::error!("Failed to disable mouse capture: {}", e);
        }
        ratatui::restore();
    });

    let mut app = App::new()?;
    // draw 1st frame
    app.draw(&mut terminal)?;
    // run event-driven main loop of app
    app.run(&mut terminal).await
}