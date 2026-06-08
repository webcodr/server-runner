use anyhow::bail;
use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use std::io::{self, Write};
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
pub struct ServerProcess {
    pub name: String,
    #[allow(dead_code)] // read by AppState in Task 9
    pub log: Arc<Mutex<RingBuffer>>,
    child: AsyncGroupChild,
}

impl ServerProcess {
    /// Spawn the server as a process group and start capturing its stdout/stderr.
    pub fn spawn(name: &str, command: &str) -> anyhow::Result<Self> {
        let mut cmd = build_command(command)?;
        let mut child = cmd.group_spawn()?;
        let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));

        // Take the piped streams from the inner tokio Child before handing
        // ownership of `child` to the struct. `.inner()` gives `&mut Child`.
        if let Some(stdout) = child.inner().stdout.take() {
            spawn_reader(stdout, Arc::clone(&log), false);
        }
        if let Some(stderr) = child.inner().stderr.take() {
            spawn_reader(stderr, Arc::clone(&log), true);
        }

        Ok(Self {
            name: name.to_string(),
            log,
            child,
        })
    }

    /// Kill the process group (kill includes wait internally).
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        self.child
            .kill()
            .await
            .map_err(|_| anyhow::anyhow!("Failed to stop process {}", self.name))
    }
}

fn spawn_reader<R>(mut stream: R, log: Arc<Mutex<RingBuffer>>, stderr: bool)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = [0; 8192];
        let mut line = String::new();

        while let Ok(n) = stream.read(&mut buf).await {
            if n == 0 {
                break;
            }

            write_output(&buf[..n], stderr);

            capture_lines(&buf[..n], &mut line, &log);
        }

        if !line.is_empty()
            && let Ok(mut buf) = log.lock()
        {
            buf.push(std::mem::take(&mut line));
        }
    });
}

fn write_output(bytes: &[u8], stderr: bool) {
    if stderr {
        let mut stream = io::stderr().lock();
        let _ = stream.write_all(bytes);
        let _ = stream.flush();
    } else {
        let mut stream = io::stdout().lock();
        let _ = stream.write_all(bytes);
        let _ = stream.flush();
    }
}

fn capture_lines(bytes: &[u8], line: &mut String, log: &Arc<Mutex<RingBuffer>>) {
    for ch in String::from_utf8_lossy(bytes).chars() {
        if ch == '\n' {
            if let Ok(mut buf) = log.lock() {
                buf.push(line.trim_end_matches('\r').to_string());
            }
            line.clear();
        } else {
            line.push(ch);
        }
    }
}
