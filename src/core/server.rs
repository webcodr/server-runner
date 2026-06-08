use anyhow::bail;
use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use std::sync::{Arc, Mutex};

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
#[allow(dead_code)] // consumed in Task 8
pub struct ServerProcess {
    pub name: String,
    pub log: Arc<Mutex<RingBuffer>>,
    child: AsyncGroupChild,
}

impl ServerProcess {
    /// Spawn the server as a process group and start capturing its stdout/stderr.
    #[allow(dead_code)] // consumed in Task 8
    pub fn spawn(name: &str, command: &str) -> anyhow::Result<Self> {
        let mut cmd = build_command(command)?;
        let mut child = cmd.group_spawn()?;
        let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));

        // Take the piped streams from the inner tokio Child before handing
        // ownership of `child` to the struct. `.inner()` gives `&mut Child`.
        if let Some(stdout) = child.inner().stdout.take() {
            spawn_reader(stdout, Arc::clone(&log));
        }
        if let Some(stderr) = child.inner().stderr.take() {
            spawn_reader(stderr, Arc::clone(&log));
        }

        Ok(Self {
            name: name.to_string(),
            log,
            child,
        })
    }

    /// Kill the process group (kill includes wait internally).
    #[allow(dead_code)] // consumed in Task 8
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        self.child
            .kill()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to stop process {}: {}", self.name, e))
    }
}

fn spawn_reader<R>(stream: R, log: Arc<Mutex<RingBuffer>>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(mut buf) = log.lock() {
                buf.push(line);
            }
        }
    });
}
