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

use anyhow::Result;
use app::App;

#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = ratatui::init();

    let mut app = App::new()?;
    let app_result = app.run(&mut terminal).await;

    ratatui::restore();
    app_result
}
