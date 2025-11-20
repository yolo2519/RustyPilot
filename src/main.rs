//! Main entry point for RustyTerm application.
//! 
//! This file initializes the TUI terminal, creates the application state,
//! runs the main event loop, and handles graceful terminal restoration on exit.

mod ui;
mod shell;
mod ai;
mod context;
mod security;
mod utils;

use anyhow::Result;
use ui::app::App;
use ui::terminal;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志（可选）
    utils::logger::init_logging();

    // 初始化终端 TUI
    let mut terminal = terminal::init_terminal()?;

    // 初始化应用状态
    let mut app = App::new()?;

    // 主事件循环（同步 or 异步都可以，这里预留异步接口）
    ui::event_loop::run(&mut terminal, &mut app).await?;

    // 恢复终端
    terminal::restore_terminal()?;
    Ok(())
}
