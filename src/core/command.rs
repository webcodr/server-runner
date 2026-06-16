use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio::task::JoinHandle;

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use crate::core::server::{LOG_CAPACITY, build_command};
use crate::core::state::RingBuffer;

/// A spawned final command plus its captured output log.
pub struct FinalCommand {
    pub child: Child,
    #[allow(dead_code)] // read by AppState in Task 9
    pub log: Arc<Mutex<RingBuffer>>,
    pub readers: Vec<JoinHandle<()>>,
}

/// Spawn the final command (NOT as a process group — matches today's `Command::spawn`).
pub fn spawn(command: &str) -> anyhow::Result<FinalCommand> {
    spawn_inner(command, true)
}

#[allow(dead_code)] // used by TUI command execution in Task 7
pub fn spawn_captured(command: &str) -> anyhow::Result<FinalCommand> {
    spawn_inner(command, false)
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

fn spawn_reader<R>(
    mut stream: R,
    log: Arc<Mutex<RingBuffer>>,
    stderr: bool,
    tee_output: bool,
) -> JoinHandle<()>
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

            if tee_output {
                write_output(&buf[..n], stderr);
            }

            capture_lines(&buf[..n], &mut line, &log);
        }

        if !line.is_empty()
            && let Ok(mut buf) = log.lock()
        {
            buf.push(std::mem::take(&mut line));
        }
    })
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

#[cfg(test)]
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
