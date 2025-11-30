//! Shell subprocess management and command execution.
//!
//! This module manages the shell subprocess lifecycle, sends commands
//! to the shell, and reads output for display in the UI.

use anyhow::Result;
use tokio::sync::mpsc::{self, Sender, Receiver};

use crate::event::AppEvent;
pub struct ShellManager {
    _event_sink: Sender<AppEvent>,   // Handle for sending message back to App, may be useful
    _pty_output: Sender<Vec<u8>>,    // Handle for sending bytes from pty reader to terminal

    // NOTE(yushun.tang): I'm not sure if ShellManager should hold these handles,
    // maybe you want to send it to another thread that will be working on reading data from pty reader.
    // totally depends on your implementation.
    // The _pty_output will be used to send chunks of bytes directly
    // to terminal renderer
    // the _event_sink will be used to inform main App something, but I'm not sure now.
    // For example, we may want to tell App that "we have read + sent some bytes from pty reader,
    // and likely the terminal is going to be updated" in event-driven design.
}

impl ShellManager {
    pub fn new(event_sink: Sender<AppEvent>) -> Result<(Self, Receiver<Vec<u8>>)> {
        // TODO: use portable_pty / tokio::process to implement real shell logic

        // TODO: we may need to have a convention on how many bytes
        //       we can send using a single Vec<u8> to balance latency and throughput
        let (tx, rx) = mpsc::channel(1024);
        // at here we spawn reader thread, pty, ...
        Ok((
            Self {
                _event_sink: event_sink,
                _pty_output: tx,
            },
            rx
        ))
    }

    // TODO：send command to shell
    pub fn send_command(&mut self, _cmd: &str) -> Result<()> {
        // TODO: do it later
        Ok(())
    }

    // TODO: read output from shell
    pub fn read_output(&mut self) -> Result<Option<String>> {
        // TODO: do it later
        Ok(None)
    }
}
