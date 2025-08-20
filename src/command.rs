use anyhow::bail;
use std::process::{Child, Command, Output, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use crate::constants::WINDOWS_CREATE_NO_WINDOW;

fn setup_command(command: &str) -> anyhow::Result<Command> {
    let command_parts = shlex::split(command)
        .ok_or_else(|| anyhow::anyhow!("Invalid command: {}", command))?;

    if command_parts.is_empty() {
        bail!("Empty command provided");
    }

    let mut cmd = Command::new(&command_parts[0]);

    for part in command_parts.iter().skip(1) {
        cmd.arg(part);
    }

    #[cfg(windows)]
    {
        cmd.creation_flags(WINDOWS_CREATE_NO_WINDOW);
    }

    Ok(cmd)
}

pub fn spawn_command(command: &str) -> anyhow::Result<Child> {
    let mut cmd = setup_command(command)?;
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    Ok(cmd.spawn()?)
}

pub fn execute_command(command: &str) -> anyhow::Result<Output> {
    let mut cmd = setup_command(command)?;
    Ok(cmd.output()?)
}