//! Main entry point for RustyTerm application.
//!
//! This file initializes the TUI terminal, creates the application state,
//! runs the main event loop, and handles graceful terminal restoration on exit.

pub mod ui;
pub mod shell;
pub mod ai;
pub mod context;
pub mod security;
pub mod utils;
pub mod app;
pub mod event;

use anyhow::Result;
use app::App;

use crate::utils::context::Context;

#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    let _ctx = Context::with(ratatui::restore);
    let mut app = App::new()?;
    // draw 1st frame
    app.draw(&mut terminal)?;
    // run event-driven main loop of app
    app.run(&mut terminal).await
}
