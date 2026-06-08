use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

use std::sync::{Arc, Mutex};

use crate::core::server::{LOG_CAPACITY, build_command};
use crate::core::state::RingBuffer;

/// A spawned final command plus its captured output log.
#[allow(dead_code)] // consumed in Task 8
pub struct FinalCommand {
    pub child: Child,
    pub log: Arc<Mutex<RingBuffer>>,
}

/// Spawn the final command (NOT as a process group — matches today's `Command::spawn`).
#[allow(dead_code)] // consumed in Task 8
pub fn spawn(command: &str) -> anyhow::Result<FinalCommand> {
    let mut cmd = build_command(command)?;
    let mut child = cmd.spawn()?;
    let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));

    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, Arc::clone(&log));
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, Arc::clone(&log));
    }

    Ok(FinalCommand { child, log })
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
