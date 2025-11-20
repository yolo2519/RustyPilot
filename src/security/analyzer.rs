use super::CommandSafety;

pub fn analyze_command(cmd: &str) -> CommandSafety {
    // TODO: Implement command analysis logic
    let trimmed = cmd.trim();

    if trimmed.contains("rm -rf /") {
        return CommandSafety::Block("Command contains 'rm -rf /'".into());
    }

    if trimmed.starts_with("rm ") {
        return CommandSafety::Warn("Command uses 'rm', please confirm.".into());
    }

    CommandSafety::Safe
}
