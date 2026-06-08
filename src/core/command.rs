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
    let mut cmd = build_command(command)?;
    let mut child = cmd.spawn()?;
    let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));
    let mut readers = Vec::new();

    if let Some(stdout) = child.stdout.take() {
        readers.push(spawn_reader(stdout, Arc::clone(&log), false));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(spawn_reader(stderr, Arc::clone(&log), true));
    }

    Ok(FinalCommand {
        child,
        log,
        readers,
    })
}

fn spawn_reader<R>(mut stream: R, log: Arc<Mutex<RingBuffer>>, stderr: bool) -> JoinHandle<()>
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
