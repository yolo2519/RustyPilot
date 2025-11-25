use anyhow::Result;
use crossterm::event::KeyEvent;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use vt100::Parser;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct ShellManager {
    pub parser: Parser,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    pty_master: Box<dyn MasterPty + Send>,
}

impl ShellManager {
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        let pty_system = native_pty_system();

        // 1. Create PTY pair
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // 2. Spawn zsh inside slave
        let mut cmd = CommandBuilder::new("zsh");
        cmd.env("TERM", "xterm-256color");

        let _child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave); // Required

        // 3. Split writer + reader
        let writer = pair.master.take_writer()?;
        let reader = pair.master.try_clone_reader()?;
        let pty_master = pair.master;

        // 4. Setup MPSC channel
        let (tx, rx) = mpsc::channel::<Vec<u8>>();

        // 5. Spawn thread to read PTY output
        thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        if tx.send(data).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // 6. VT100 parser (scrollback = 2000 lines)
        let parser = Parser::new(rows, cols, 2000);

        Ok(Self {
            parser,
            writer,
            output_rx: rx,
            pty_master,
        })
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.pty_master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        self.parser = Parser::new(rows, cols, 2000);
        Ok(())
    }

    pub fn read_output(&mut self) {
        while let Ok(data) = self.output_rx.try_recv() {
            self.parser.process(&data);
        }
    }

    pub fn send_key(&mut self, key: KeyEvent) -> Result<()> {
        let bytes = key_to_bytes(key);
        self.writer.write_all(&bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn lines(&self) -> Vec<Line<'_>> {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();

        let mut lines = vec![];

        for row in 0..rows {
            let mut spans = vec![];
            let mut curr = String::new();
            let mut curr_style = Style::default();

            for col in 0..cols {
                let cell = match screen.cell(row, col) {
                    Some(c) => c,
                    None => continue,
                };

                let text = cell.contents();
                let mut style = Style::default();

                style = style.fg(convert_color(cell.fgcolor()));
                style = style.bg(convert_color(cell.bgcolor()));

                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.underline() {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.italic() {
                    style = style.add_modifier(Modifier::ITALIC);
                }

                if style == curr_style {
                    curr.push_str(&text);
                } else {
                    if !curr.is_empty() {
                        spans.push(Span::styled(curr.clone(), curr_style));
                        curr.clear();
                    }
                    curr_style = style;
                    curr.push_str(&text);
                }
            }

            if !curr.is_empty() {
                spans.push(Span::styled(curr, curr_style));
            }

            lines.push(Line::from(spans));
        }

        lines
    }
}

fn convert_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => match i {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            7 => Color::Gray,
            _ => Color::Indexed(i),
        },
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn key_to_bytes(event: KeyEvent) -> Vec<u8> {
    use crossterm::event::{KeyCode, KeyModifiers};

    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);

    match event.code {
        KeyCode::Char(c) if ctrl => vec![(c as u8 - b'a' + 1)],
        KeyCode::Char(c) => vec![c as u8],
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => vec![0x1b, b'[', b'D'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        _ => vec![],
    }
}

// unit tests
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn test_key_to_bytes_plain_char() {
        let k = key(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key_to_bytes(k), vec![b'a']);
    }

    #[test]
    fn test_key_to_bytes_ctrl_char() {
        let k = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        // Ctrl+C should be 3
        assert_eq!(key_to_bytes(k), vec![3]);
    }

    #[test]
    fn test_key_to_bytes_enter_and_backspace() {
        let enter = key(KeyCode::Enter, KeyModifiers::NONE);
        let backspace = key(KeyCode::Backspace, KeyModifiers::NONE);

        assert_eq!(key_to_bytes(enter), vec![b'\r']);
        assert_eq!(key_to_bytes(backspace), vec![0x7f]);
    }

    #[test]
    fn test_key_to_bytes_arrows() {
        let up = key(KeyCode::Up, KeyModifiers::NONE);
        let down = key(KeyCode::Down, KeyModifiers::NONE);

        assert_eq!(key_to_bytes(up), vec![0x1b, b'[', b'A']);
        assert_eq!(key_to_bytes(down), vec![0x1b, b'[', b'B']);
    }
}
