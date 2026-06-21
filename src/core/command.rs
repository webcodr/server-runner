use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::process::Child;
use tokio::task::JoinHandle;

use std::sync::{Arc, Mutex};

use crate::core::output::spawn_reader;
use crate::core::server::{LOG_CAPACITY, build_command};
use crate::core::state::RingBuffer;

/// A spawned final command plus its captured output log.
pub struct FinalCommand {
    pub child: Child,
    #[allow(dead_code)] // read by AppState in Task 9
    pub log: Arc<Mutex<RingBuffer>>,
    pub readers: Vec<JoinHandle<()>>,
}

/// A captured final command spawned as a process group for TUI cancellation.
pub struct FinalCommandGroup {
    child: AsyncGroupChild,
    pub log: Arc<Mutex<RingBuffer>>,
    pub readers: Vec<JoinHandle<()>>,
}

impl FinalCommandGroup {
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    pub async fn cancel(&mut self) -> std::io::Result<()> {
        self.child.kill().await
    }
}

/// Spawn the final command (NOT as a process group — matches today's `Command::spawn`).
pub fn spawn(command: &str) -> anyhow::Result<FinalCommand> {
    spawn_inner(command, true)
}

#[allow(dead_code)] // used by TUI command execution in Task 7
pub fn spawn_captured(command: &str) -> anyhow::Result<FinalCommand> {
    spawn_inner(command, false)
}

#[allow(dead_code)] // used by TUI command execution
pub fn spawn_captured_group(command: &str) -> anyhow::Result<FinalCommandGroup> {
    let mut cmd = build_command(command)?;
    let mut child = cmd.group_spawn()?;
    let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));
    let mut readers = Vec::new();

    if let Some(stdout) = child.inner().stdout.take() {
        readers.push(spawn_reader(stdout, Arc::clone(&log), false, false));
    }
    if let Some(stderr) = child.inner().stderr.take() {
        readers.push(spawn_reader(stderr, Arc::clone(&log), true, false));
    }

    Ok(FinalCommandGroup {
        child,
        log,
        readers,
    })
}

fn spawn_inner(command: &str, tee_output: bool) -> anyhow::Result<FinalCommand> {
    let mut cmd = build_command(command)?;
    if !tee_output {
        cmd.kill_on_drop(true);
    }

    let mut child = cmd.spawn()?;
    let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));
    let mut readers = Vec::new();

    if let Some(stdout) = child.stdout.take() {
        readers.push(spawn_reader(stdout, Arc::clone(&log), false, tee_output));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(spawn_reader(stderr, Arc::clone(&log), true, tee_output));
    }

    Ok(FinalCommand {
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
        let mut command = spawn_captured("sh -c 'echo out; echo err >&2'").unwrap();
        let status = command.child.wait().await.unwrap();
        for reader in command.readers {
            let _ = reader.await;
        }

        assert!(status.success());
        let lines: Vec<_> = command.log.lock().unwrap().iter().cloned().collect();
        assert!(lines.contains(&"out".to_string()));
        assert!(lines.contains(&"err".to_string()));
    }
}
