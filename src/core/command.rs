use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::task::JoinHandle;

use std::sync::{Arc, Mutex};

use crate::core::output::spawn_reader;
use crate::core::server::{LOG_CAPACITY, build_command};
use crate::core::state::RingBuffer;

/// A spawned final command plus its captured output log.
///
/// Always a process group, so the command and every descendant it spawned can
/// be killed together on shutdown.
pub struct FinalCommandGroup {
    child: AsyncGroupChild,
    pub log: Arc<Mutex<RingBuffer>>,
    pub readers: Vec<JoinHandle<()>>,
}

impl FinalCommandGroup {
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    /// Kill the whole process group, descendants included.
    pub async fn cancel(&mut self) -> std::io::Result<()> {
        self.child.kill().await
    }
}

/// Spawn the final command as a process group.
///
/// `tee_output` mirrors the child's output to the real stdout/stderr for plain
/// mode; the TUI captures only and renders the log itself.
pub fn spawn_group(command: &str, tee_output: bool) -> anyhow::Result<FinalCommandGroup> {
    let mut cmd = build_command(command)?;
    let mut child = cmd.group_spawn()?;
    let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));
    let mut readers = Vec::new();

    if let Some(stdout) = child.inner().stdout.take() {
        readers.push(spawn_reader(stdout, Arc::clone(&log), false, tee_output));
    }
    if let Some(stderr) = child.inner().stderr.take() {
        readers.push(spawn_reader(stderr, Arc::clone(&log), true, tee_output));
    }

    Ok(FinalCommandGroup {
        child,
        log,
        readers,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_captured_records_output_lines() {
        let mut command = spawn_group("sh -c 'echo out; echo err >&2'", false).unwrap();
        let status = command.wait().await.unwrap();
        for reader in command.readers {
            let _ = reader.await;
        }

        assert!(status.success());
        let lines: Vec<_> = command.log.lock().unwrap().iter().cloned().collect();
        assert!(lines.contains(&"out".to_string()));
        assert!(lines.contains(&"err".to_string()));
    }
}
