//! Shell subprocess management and command execution.
//!
//! This module manages the shell subprocess lifecycle, sends commands
//! to the shell, and reads output for display in the UI.

use anyhow::Result;
use tokio::sync::mpsc::{self, Sender, Receiver, UnboundedSender};

use crate::event::AppEvent;
pub struct ShellManager {
    _event_sink: UnboundedSender<AppEvent>,   // Handle for sending message back to App, may be useful
    pty_output: Sender<Vec<u8>>,    // Handle for sending bytes from pty reader to terminal

    // NOTE(yushun.tang): I'm not sure if ShellManager should hold these handles,
    // maybe you want to send it to another thread that will be working on reading data from pty reader.
    // totally depends on your implementation.
    // The pty_output will be used to send chunks of bytes directly
    // to terminal renderer
    // the event_sink will be used to inform main App something, but I'm not sure now.
    // For example, we may want to tell App that "we have read + sent some bytes from pty reader,
    // and likely the terminal is going to be updated" in event-driven design.
}

impl ShellManager {
    pub fn new(event_sink: UnboundedSender<AppEvent>) -> Result<(Self, Receiver<Vec<u8>>)> {
        // TODO: use portable_pty / tokio::process to implement real shell logic

        // TODO: we may need to have a convention on how many bytes
        //       we can send using a single Vec<u8> to balance latency and throughput
        let (tx, rx) = mpsc::channel(1024);
        // at here we spawn reader thread, pty, ...
        Ok((
            Self {
                _event_sink: event_sink,
                pty_output: tx,
            },
            rx
        ))
    }

    /// Handle user input - for dummy implementation, echo it to pty_output
    pub fn handle_user_input(&mut self, input: &str) -> Result<()> {
        // Dummy: Echo user input to pty_output stream
        // TODO: this is fake
        let output = input.to_string();
        let tx = self.pty_output.clone();
        // in the real implementation the pty_writer's write method will be called so there should
        // be no futures. If the writer could be async, we will have to refactor our tokio::select
        tokio::spawn( async move {
            #[allow(clippy::unwrap_used, reason = "This is a fake implementation")]
            tx.send(output.into_bytes()).await.unwrap();
        });
        Ok(())
    }

    /// Inject a command into the shell as if the user typed it.
    /// This will append the command to the PTY input buffer and execute it.
    pub fn inject_command(&mut self, cmd: &str) -> Result<()> {
        // Dummy: Echo the command to pty_output stream
        // TODO: this is fake
        let tx = self.pty_output.clone();
        let cmd = cmd.to_string();
        // in the real implementation the pty_writer's write method will be called so there should
        // be no futures. If the writer could be async, we will have to refactor our tokio::select
        tokio::spawn( async move {
            #[allow(clippy::unwrap_used, reason = "This is a fake implementation")]
            tx.send(cmd.into_bytes()).await.unwrap();
        });
        Ok(())
    }

    // TODO: read output from shell
    pub fn read_output(&mut self) -> Result<Option<String>> {
        // TODO: do it later
        Ok(None)
    }
}
