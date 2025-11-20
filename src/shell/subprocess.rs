use anyhow::Result;
pub struct ShellManager;

impl ShellManager {
    pub fn new() -> Result<Self> {
        // TODO: use portable_pty / tokio::process to implement real shell logic
        Ok(Self)
    }

    // TODO：send command to shell
    pub fn send_command(&mut self, _cmd: &str) -> Result<()> {
        // TODO: 后面实现
        Ok(())
    }

    // TODO: read output from shell
    pub fn read_output(&mut self) -> Result<Option<String>> {
        // TODO: 后面实现
        Ok(None)
    }
}
