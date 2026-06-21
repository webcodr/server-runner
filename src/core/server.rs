use anyhow::bail;
use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::process::Command;

use std::sync::{Arc, Mutex};

use crate::core::output::spawn_reader;
use crate::core::state::RingBuffer;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub const LOG_CAPACITY: usize = 5000;

/// Build a tokio `Command` from a shell-like command string, with stdout/stderr piped.
pub fn build_command(command: &str) -> anyhow::Result<Command> {
    let parts =
        shlex::split(command).ok_or_else(|| anyhow::anyhow!("Invalid command: {}", command))?;
    if parts.is_empty() {
        bail!("Empty command provided");
    }

    let mut cmd = Command::new(&parts[0]);
    for part in parts.iter().skip(1) {
        cmd.arg(part);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    Ok(cmd)
}

/// A running server process group plus its captured log buffer.
pub struct ServerProcess {
    pub name: String,
    #[allow(dead_code)] // read by AppState in Task 9
    pub log: Arc<Mutex<RingBuffer>>,
    child: AsyncGroupChild,
}

impl ServerProcess {
    /// Spawn the server as a process group and start capturing its stdout/stderr.
    pub fn spawn(name: &str, command: &str) -> anyhow::Result<Self> {
        Self::spawn_inner(name, command, true)
    }

    #[allow(dead_code)] // used by TUI server execution in Task 7
    pub fn spawn_captured(name: &str, command: &str) -> anyhow::Result<Self> {
        Self::spawn_inner(name, command, false)
    }

    fn spawn_inner(name: &str, command: &str, tee_output: bool) -> anyhow::Result<Self> {
        let mut cmd = build_command(command)?;
        let mut child = cmd.group_spawn()?;
        let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));

        // Take the piped streams from the inner tokio Child before handing
        // ownership of `child` to the struct. `.inner()` gives `&mut Child`.
        if let Some(stdout) = child.inner().stdout.take() {
            spawn_reader(stdout, Arc::clone(&log), false, tee_output);
        }
        if let Some(stderr) = child.inner().stderr.take() {
            spawn_reader(stderr, Arc::clone(&log), true, tee_output);
        }

        Ok(Self {
            name: name.to_string(),
            log,
            child,
        })
    }

    /// Process-group leader PID, if still available.
    #[allow(dead_code)] // used by TUI control-command tests
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// Kill the process group (kill includes wait internally).
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        self.child
            .kill()
            .await
            .map_err(|_| anyhow::anyhow!("Failed to stop process {}", self.name))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_captured_server_records_output_lines() {
        let mut server =
            ServerProcess::spawn_captured("Test", "sh -c 'echo server-out; sleep 5'").unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        server.stop().await.unwrap();

        let lines: Vec<_> = server.log.lock().unwrap().iter().cloned().collect();
        assert!(lines.contains(&"server-out".to_string()));
    }
}
