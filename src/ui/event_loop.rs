//! Main event loop for the TUI application.
//! 
//! This module runs the main event loop, handling user input (keyboard events),
//! rendering the UI, and coordinating between different panels. It manages
//! the application lifecycle until the user exits.

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::Terminal;

use crate::ui::terminal::TuiTerminal;
use crate::ui::{ai_panel, app::App, layout, shell_panel};

pub async fn run(terminal: &mut TuiTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| {
            let size = f.area();
            let [shell_area, ai_area] = layout::Layouts::main_chunks(size);

            shell_panel::render_shell_pane(f, shell_area, app);
            ai_panel::render_ai_pane(f, ai_area, app);
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => break,       // exit
                    KeyCode::Tab => app.switch_pane(), // switch
                    // TODO
                    _ => {}
                },
                _ => {}
            }
        }
        // TODO: poll async AI results
    }

    Ok(())
}
