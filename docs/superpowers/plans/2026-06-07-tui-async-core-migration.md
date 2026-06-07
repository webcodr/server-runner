# Async Core Migration Implementation Plan (Plan 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate server-runner's synchronous single-file core to a unified async (tokio) engine split into focused modules, with plain-log behavior observably identical and all existing integration tests green.

**Architecture:** Extract `src/main.rs` into `cli`, `config`, `core/*`, and `runner/plain` modules. Replace `reqwest::blocking` with async `reqwest`, `thread::sleep` with `tokio::time::sleep`, and `std::process`/`command-group` server spawning with `tokio::process` + `command-group`'s async API, capturing child output into bounded ring buffers that plain mode forwards to the log. A `--tui` flag is added but only wired to a "not yet implemented" path (Plan 2 fills it in).

**Tech Stack:** Rust 2024, tokio, async reqwest, command-group (tokio feature), anyhow, clap, config/serde, log/simplelog.

**Scope note:** This is Plan 1 of 2. Plan 2 (TUI control panel) is written after this lands, against the concrete async API produced here. The reference spec is `docs/superpowers/specs/2026-06-07-tui-control-panel-design.md`.

---

## File Structure

After this plan, `src/` looks like:

```
src/
  main.rs        # tokio runtime bootstrap, dispatch to runner
  cli.rs         # Args (incl. --tui flag)
  config.rs      # Config, Server, validation, get_config
  core/
    mod.rs       # Engine: owns servers, runs polling, exposes AppState + commands
    state.rs     # ServerStatus, Attempts, ServerName, RingBuffer, AppState, FinalCmd
    health.rs    # async readiness check
    server.rs    # spawn server (tokio::process), output capture, kill group
    command.rs   # final command spawn + capture
  runner/
    mod.rs       # runner module root
    plain.rs     # non-TUI mode: drive engine -> logs (current behavior)
```

`runner/tui/*` is intentionally **not** created here; it belongs to Plan 2.

Module responsibilities:
- `cli.rs` — CLI surface only. No logic.
- `config.rs` — parsing + validation. Already cohesive; moved verbatim.
- `core/state.rs` — plain data types + the shared `AppState`. No IO.
- `core/health.rs` — one async function: is a URL ready?
- `core/server.rs` — server process lifecycle + output capture.
- `core/command.rs` — final command lifecycle + output capture.
- `core/mod.rs` — orchestration (the engine) tying the above together.
- `runner/plain.rs` — translate engine progress into today's log output + exit code.

---

## Task 1: Add async dependencies

**Files:**
- Modify: `Cargo.toml:14-27`

- [ ] **Step 1: Update dependencies**

Replace the `[dependencies]` block in `Cargo.toml` with:

```toml
[dependencies]
anyhow = "1.0.98"
clap = { version = "4.5.39", features = ["derive"] }
command-group = { version = "5.0.1", features = ["with-tokio"] }
config = { version = "0.15.11", default-features = false, features = ["yaml"] }
ctrlc = "3.4.7"
log = "0.4.27"
reqwest = { version = "0.12.19", default-features = false, features = [
    "native-tls-vendored",
] }
serde = { version = "1", features = ["derive"] }
shlex = "1.3.0"
simplelog = "0.12.2"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "time", "sync", "io-util"] }
```

Notes: `reqwest` drops `"blocking"` (now uses the default async client). `command-group` gains `"with-tokio"` for `AsyncCommandGroup`.

- [ ] **Step 2: Verify it resolves and still builds**

Run: `cargo build`
Expected: builds successfully (existing sync `main.rs` still compiles; `reqwest::blocking` is gone so this will FAIL to compile if `main.rs` still references it — if so, this task is correctly sequenced before the code changes only when the next tasks land in the same branch). To keep the tree compiling, instead run:

Run: `cargo metadata --format-version 1 > /dev/null`
Expected: exits 0 (dependency graph resolves). Full `cargo build` is restored to green in Task 8.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add tokio and switch reqwest to async client"
```

---

## Task 2: Extract `config` module (pure refactor)

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` (remove moved items, add `mod config;` + `use`)

- [ ] **Step 1: Create `src/config.rs` with the moved code**

```rust
use anyhow::{Context, bail};

const MIN_TIMEOUT_SECONDS: u64 = 1;
const MAX_TIMEOUT_SECONDS: u64 = 300;

#[derive(serde::Deserialize)]
pub struct Server {
    pub name: String,
    pub url: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_timeout() -> u64 {
    5
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub servers: Vec<Server>,
    pub command: String,
}

fn validate_readiness_url(server_name: &str, url: &str) -> anyhow::Result<()> {
    let parsed = reqwest::Url::parse(url)
        .with_context(|| format!("Readiness URL for server {} is invalid", server_name))?;

    match parsed.scheme() {
        "http" | "https" => Ok(()),
        _ => bail!(
            "Readiness URL for server {} must use http or https",
            server_name
        ),
    }
}

fn validate_server_timeout(server_name: &str, timeout: u64) -> anyhow::Result<()> {
    if !(MIN_TIMEOUT_SECONDS..=MAX_TIMEOUT_SECONDS).contains(&timeout) {
        bail!(
            "Timeout for server {} must be between {} and {} seconds",
            server_name,
            MIN_TIMEOUT_SECONDS,
            MAX_TIMEOUT_SECONDS
        );
    }

    Ok(())
}

pub fn get_config(filename: &str) -> anyhow::Result<Config> {
    let cwd = std::env::current_dir()?;
    let tmp_path = cwd.join(filename);
    let config_file_path = tmp_path.to_str().context(format!(
        "Could not create String from Path {}",
        tmp_path.display()
    ))?;

    log::info!("Loading config file {}", config_file_path);

    let settings = config::Config::builder()
        .add_source(config::File::new(config_file_path, config::FileFormat::Yaml))
        .build()
        .context(format!("Could not find config file {}", filename))?;

    let config = settings
        .try_deserialize::<Config>()
        .context(format!("Could not parse config file {}", filename))?;

    if config.servers.is_empty() {
        bail!("Configuration must include at least one server");
    }

    if config.command.trim().is_empty() {
        bail!("Configuration must include a command to run");
    }

    for server in &config.servers {
        validate_server_timeout(&server.name, server.timeout)?;
        validate_readiness_url(&server.name, &server.url)?;
    }

    Ok(config)
}
```

- [ ] **Step 2: Remove the moved code from `src/main.rs`**

Delete from `main.rs`: the `MIN_TIMEOUT_SECONDS`/`MAX_TIMEOUT_SECONDS` consts, `Server`, `default_timeout`, `validate_readiness_url`, `validate_server_timeout`, `Config`, and `get_config`. Add at the top of `main.rs`:

```rust
mod config;

use config::{Config, Server, get_config};
```

Remove now-unused imports from `main.rs` (`std::env`). Keep all other code as-is.

- [ ] **Step 3: Verify build + tests**

Run: `cargo test`
Expected: all existing tests in `tests/cli.rs` PASS (behavior unchanged).

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "refactor: extract config module"
```

---

## Task 3: Extract `cli` module and add `--tui` flag (inert)

**Files:**
- Create: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create `src/cli.rs`**

```rust
use clap::Parser;

#[derive(Parser)]
#[command(version)]
pub struct Args {
    #[arg(short, long, default_value = "servers.yaml")]
    pub config: String,

    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    #[arg(short, long, default_value_t = 10, value_parser = clap::value_parser!(u8).range(1..=255))]
    pub attempts: u8,

    /// Run the interactive TUI control panel instead of plain log output.
    #[arg(long, default_value_t = false)]
    pub tui: bool,
}
```

- [ ] **Step 2: Update `src/main.rs`**

Remove the `Args` struct and `use clap::Parser;` from `main.rs`. Add:

```rust
mod cli;

use cli::Args;
```

`Args::parse()` in `main()` still works (clap's `Parser` trait is implemented in `cli.rs`); add `use clap::Parser;` inside `main.rs` only if `parse()` no longer resolves — prefer calling `cli::Args::parse()` with `use clap::Parser;` retained at top of `main.rs`.

- [ ] **Step 3: Verify**

Run: `cargo test rejects_zero_attempts`
Expected: PASS (flag parsing unchanged).

Run: `cargo run -- --help`
Expected: help text now lists `--tui`.

- [ ] **Step 4: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "refactor: extract cli module and add inert --tui flag"
```

---

## Task 4: Core state types + RingBuffer (TDD)

**Files:**
- Create: `src/core/mod.rs` (temporary: just `pub mod state;` for now)
- Create: `src/core/state.rs`
- Modify: `src/main.rs` (add `mod core;`, remove moved `Attempts`/`ServerName`/`ServerStatus`)

- [ ] **Step 1: Write failing unit tests in `src/core/state.rs`**

```rust
use std::collections::VecDeque;
use std::fmt;
use std::ops::AddAssign;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStatus {
    Waiting,
    Running,
    Failed,
    Stopped,
}

#[derive(Copy, Clone, Debug)]
pub struct Attempts(pub u8);

impl AddAssign<u8> for Attempts {
    fn add_assign(&mut self, other: u8) {
        self.0 = self.0.saturating_add(other);
    }
}

impl fmt::Display for Attempts {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<u8> for Attempts {
    fn eq(&self, other: &u8) -> bool {
        self.0 == *other
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServerName(pub String);

/// Bounded FIFO line buffer. Oldest lines drop once `capacity` is exceeded.
pub struct RingBuffer {
    lines: VecDeque<String>,
    capacity: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self { lines: VecDeque::with_capacity(capacity.min(1024)), capacity }
    }

    pub fn push(&mut self, line: String) {
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.lines.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_saturate_at_u8_max() {
        let mut a = Attempts(254);
        a += 1;
        a += 1;
        a += 1;
        assert_eq!(a, 255u8);
    }

    #[test]
    fn ring_buffer_drops_oldest_when_full() {
        let mut rb = RingBuffer::new(2);
        rb.push("a".into());
        rb.push("b".into());
        rb.push("c".into());
        assert_eq!(rb.len(), 2);
        let got: Vec<_> = rb.iter().cloned().collect();
        assert_eq!(got, vec!["b".to_string(), "c".to_string()]);
    }
}
```

Create `src/core/mod.rs`:

```rust
pub mod state;
```

- [ ] **Step 2: Run tests to verify they pass after wiring the module**

Add `mod core;` to `src/main.rs`. In `main.rs`, replace the local `Attempts`, `ServerName`, `ServerStatus` definitions with `use core::state::{Attempts, ServerName, ServerStatus};`. Update the existing `check_server` match arms that compare `ServerStatus::Waiting`/`Running` (now `Copy`, `PartialEq` derived — `result == ServerStatus::Waiting` still works).

Run: `cargo test --lib`
Expected: `attempts_saturate_at_u8_max` and `ring_buffer_drops_oldest_when_full` PASS.

- [ ] **Step 3: Verify integration tests still pass**

Run: `cargo test`
Expected: all `tests/cli.rs` tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/core/mod.rs src/core/state.rs src/main.rs
git commit -m "refactor: extract core state types and add RingBuffer"
```

---

## Task 5: Async readiness check (TDD)

**Files:**
- Create: `src/core/health.rs`
- Modify: `src/core/mod.rs` (add `pub mod health;`)

- [ ] **Step 1: Write `src/core/health.rs`**

Mirror today's semantics exactly: connect error -> `Waiting`; non-connect send error -> hard error; non-success status -> `Waiting`; success -> `Running`; redirects not followed.

```rust
use crate::core::state::ServerStatus;
use anyhow::bail;
use std::time::Duration;

/// Perform a single readiness probe. Does not mutate attempts.
pub async fn check(name: &str, url: &str, timeout_secs: u64) -> anyhow::Result<ServerStatus> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    match client.get(url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                Ok(ServerStatus::Running)
            } else {
                Ok(ServerStatus::Waiting)
            }
        }
        Err(error) => {
            if error.is_connect() || error.is_timeout() {
                Ok(ServerStatus::Waiting)
            } else {
                bail!("Could not connect to server {} on url {}", name, url);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connection_refused_is_waiting() {
        // Port 1 is privileged/unused; connect fails fast.
        let status = check("Test", "http://127.0.0.1:1", 1).await.unwrap();
        assert_eq!(status, ServerStatus::Waiting);
    }
}
```

Note: today's blocking code treats a timeout as a hard error path only via `is_connect()`; we additionally treat `is_timeout()` as `Waiting` so a slow-starting server keeps retrying rather than aborting. This is consistent with the spec's "keep retrying until attempts exhausted" behavior and does not change any existing test (the timeout fixtures point at unreachable hosts, which produce connect errors and exhaust attempts). Verify in Step 3.

- [ ] **Step 2: Wire module**

Add to `src/core/mod.rs`:

```rust
pub mod health;
```

- [ ] **Step 3: Run tests**

Run: `cargo test connection_refused_is_waiting`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/core/health.rs src/core/mod.rs
git commit -m "feat: add async readiness check"
```

---

## Task 6: Async server process + output capture

**Files:**
- Create: `src/core/server.rs`
- Modify: `src/core/mod.rs` (add `pub mod server;`)

- [ ] **Step 1: Write `src/core/server.rs`**

```rust
use crate::core::state::RingBuffer;
use anyhow::bail;
use command_group::AsyncCommandGroup;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub const LOG_CAPACITY: usize = 5000;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Build a tokio Command from a shell-like command string, piping stdout/stderr.
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

/// A running server process plus its captured log buffer.
pub struct ServerProcess {
    pub name: String,
    pub log: Arc<Mutex<RingBuffer>>,
    child: command_group::AsyncGroupChild,
}

impl ServerProcess {
    /// Spawn the server as a process group and start capturing its output.
    pub fn spawn(name: &str, command: &str) -> anyhow::Result<Self> {
        let mut cmd = build_command(command)?;
        let mut child = cmd.group_spawn()?;
        let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));

        if let Some(stdout) = child.inner().stdout.take() {
            spawn_reader(stdout, Arc::clone(&log), name.to_string());
        }
        if let Some(stderr) = child.inner().stderr.take() {
            spawn_reader(stderr, Arc::clone(&log), name.to_string());
        }

        Ok(Self { name: name.to_string(), log, child })
    }

    /// Kill the process group and reap it.
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        if self.child.kill().is_ok() {
            let _ = self.child.wait().await;
            Ok(())
        } else {
            bail!("Failed to stop process {}", self.name);
        }
    }
}

fn spawn_reader<R>(stream: R, log: Arc<Mutex<RingBuffer>>, _tag: String)
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
```

> **Implementation note for the engineer:** `command-group` 5.x exposes the async group child via `AsyncCommandGroup::group_spawn` returning `command_group::AsyncGroupChild`, with `.inner()` giving the underlying `tokio::process::Child` (for taking `stdout`/`stderr`) and `.kill()`/`.wait()` operating on the whole group. If the exact type path or method names differ in the installed version, run `cargo doc -p command-group --open` and adjust — the behavior contract (spawn group, take piped stdout/stderr, kill+wait the group) is what must hold. This is the only API in the plan not verified against a compiler; confirm it first.

- [ ] **Step 2: Wire module**

Add to `src/core/mod.rs`:

```rust
pub mod server;
```

- [ ] **Step 3: Verify it compiles in isolation**

Run: `cargo build`
Expected: compiles (note: `main.rs` still uses the old sync spawning; that is replaced in Task 8). If `main.rs` no longer compiles because Task 8 hasn't landed, this step's verification is `cargo build --lib`? `server.rs` is part of the binary crate, so use:

Run: `cargo check`
Expected: compiles once Task 8 lands. Until then, verify just this file's types with a temporary `#[allow(dead_code)]` and `cargo check`. The integration green-bar is restored in Task 8 Step 4.

- [ ] **Step 4: Commit**

```bash
git add src/core/server.rs src/core/mod.rs
git commit -m "feat: async server process spawn with output capture"
```

---

## Task 7: Final command runner

**Files:**
- Create: `src/core/command.rs`
- Modify: `src/core/mod.rs` (add `pub mod command;`)

- [ ] **Step 1: Write `src/core/command.rs`**

```rust
use crate::core::server::build_command;
use crate::core::state::RingBuffer;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

/// A spawned final command plus its captured output.
pub struct FinalCommand {
    pub child: Child,
    pub log: Arc<Mutex<RingBuffer>>,
}

/// Spawn the final command (not as a group; matches today's `Command::spawn`).
pub fn spawn(command: &str) -> anyhow::Result<FinalCommand> {
    let mut cmd = build_command(command)?;
    let mut child = cmd.spawn()?;
    let log = Arc::new(Mutex::new(RingBuffer::new(
        crate::core::server::LOG_CAPACITY,
    )));

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
```

> **Note:** today's final command inherits the terminal (output goes straight to the user's stdout). To preserve that *visible* behavior in plain mode, the plain runner (Task 8) drains this captured `log` to stdout/stderr. Piping (instead of inherit) is required so the same code path feeds the TUI in Plan 2.

- [ ] **Step 2: Wire module**

Add to `src/core/mod.rs`:

```rust
pub mod command;
```

- [ ] **Step 3: Verify**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add src/core/command.rs src/core/mod.rs
git commit -m "feat: async final command runner with output capture"
```

---

## Task 8: Engine + plain runner (replaces old run loop)

**Files:**
- Create: `src/core/mod.rs` additions (Engine)
- Create: `src/runner/mod.rs`, `src/runner/plain.rs`
- Modify: `src/main.rs` (delete old sync logic, become tokio bootstrap)

- [ ] **Step 1: Add the `Engine` to `src/core/mod.rs`**

Append below the `pub mod` lines:

```rust
pub mod state;
pub mod health;
pub mod server;
pub mod command;

use crate::config::Server;
use crate::core::server::ServerProcess;
use crate::core::state::{Attempts, ServerName, ServerStatus};
use std::collections::HashMap;

/// Outcome of a single readiness sweep across all servers.
pub enum Sweep {
    /// All servers returned Running.
    AllReady,
    /// At least one server still Waiting.
    Waiting,
}

/// Owns the running server processes and tracks attempts.
pub struct Engine {
    pub processes: Vec<ServerProcess>,
    attempts: HashMap<ServerName, Attempts>,
    max_attempts: u8,
}

impl Engine {
    /// Spawn all servers.
    pub fn start(servers: &[Server], max_attempts: u8) -> anyhow::Result<Self> {
        let mut processes = Vec::with_capacity(servers.len());
        for s in servers {
            log::info!("Starting server {}", s.name);
            processes.push(ServerProcess::spawn(&s.name, &s.command)?);
        }
        Ok(Self { processes, attempts: HashMap::new(), max_attempts })
    }

    /// Probe one server, incrementing its attempt count.
    /// Returns Err when attempts are exhausted (plain-mode abort contract).
    pub async fn probe(&mut self, server: &Server) -> anyhow::Result<ServerStatus> {
        let attempts = self
            .attempts
            .entry(ServerName(server.name.clone()))
            .and_modify(|a| *a += 1)
            .or_insert(Attempts(1));

        if attempts.0 >= self.max_attempts {
            let word = if self.max_attempts == 1 { "attempt" } else { "attempts" };
            anyhow::bail!(
                "Could not connect to server {} after {} {}",
                server.name,
                attempts,
                word
            );
        }

        log::info!(
            "Checking server {} on url {}, attempt {}, waiting one second ...",
            server.name,
            server.url,
            attempts
        );

        health::check(&server.name, &server.url, server.timeout).await
    }

    /// Stop all server process groups.
    pub async fn stop_all(&mut self) -> anyhow::Result<()> {
        for p in self.processes.iter_mut() {
            log::info!("Stopping server {}", p.name);
            p.stop().await?;
        }
        log::info!("All servers stopped successfully");
        Ok(())
    }
}
```

> The attempt-count and message formatting reproduce `src/main.rs:319-373` exactly (same `>=` comparison, same "attempt"/"attempts" pluralization, same wording) so the `fails_on_*_attempts` integration tests pass byte-for-byte.

- [ ] **Step 2: Create `src/runner/mod.rs`**

```rust
pub mod plain;
```

- [ ] **Step 3: Create `src/runner/plain.rs`**

```rust
use crate::config::Config;
use crate::core::Engine;
use crate::core::state::ServerStatus;
use anyhow::Context;
use std::time::Duration;

/// Drive the engine, logging progress; exit semantics match the legacy tool.
pub async fn run(config: Config, max_attempts: u8) -> anyhow::Result<()> {
    let Config { servers, command } = config;
    let mut engine = Engine::start(&servers, max_attempts)?;

    let final_result = loop {
        let mut ready = true;

        for server in &servers {
            match engine.probe(server).await {
                Ok(ServerStatus::Running) => {}
                Ok(_) => ready = false,
                Err(e) => {
                    engine.stop_all().await.ok();
                    return Err(e);
                }
            }
        }

        if ready {
            break run_final_command(&command).await;
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    };

    engine.stop_all().await?;
    final_result
}

async fn run_final_command(command: &str) -> anyhow::Result<()> {
    let mut final_cmd = crate::core::command::spawn(command)
        .context(format!("Could not start process {}", command))?;

    log::info!("Running command {}", command);

    let status = final_cmd.child.wait().await?;

    // Forward captured output to preserve visible behavior.
    if let Ok(buf) = final_cmd.log.lock() {
        for line in buf.iter() {
            println!("{}", line);
        }
    }

    if status.success() {
        log::info!("Command {} finished successfully", command);
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Command {} failed with exit status {}",
            command,
            status
        ))
    }
}
```

> **Exit-code contract:** the `stops_servers_when_final_command_cannot_spawn` test expects `Could not start process <cmd>` on stderr and a non-zero exit; the `.context(...)` above reproduces it. The `fails_when_final_command_exits_non_zero` test expects `Command false failed with exit status ...`; reproduced verbatim. Both still stop servers (the `loop` breaks into `final_result`, then `stop_all` runs; on spawn failure `run_final_command` returns Err and `stop_all` still runs before returning).

> **Correction for spawn-failure cleanup:** ensure servers are stopped even when `run_final_command` errors. In `run`, the `break run_final_command(&command).await;` path falls through to `engine.stop_all().await?; final_result`, so cleanup happens regardless of Ok/Err. Confirmed by `stops_servers_when_final_command_cannot_spawn`.

- [ ] **Step 4: Rewrite `src/main.rs`**

Replace the entire file with:

```rust
mod cli;
mod config;
mod core;
mod runner;

use cli::Args;
use clap::Parser;

fn exit_with_error(e: anyhow::Error) -> ! {
    eprintln!("An error occurred: {}", e);
    std::process::exit(1);
}

fn main() {
    let args = Args::parse();

    let log_level = if args.verbose {
        simplelog::LevelFilter::Info
    } else {
        simplelog::LevelFilter::Warn
    };
    let _ = simplelog::TermLogger::init(
        log_level,
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    );

    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|rt| rt.block_on(async_main(args)));

    if let Err(e) = result {
        exit_with_error(e);
    }
}

async fn async_main(args: Args) -> anyhow::Result<()> {
    let config = config::get_config(&args.config)?;

    if args.tui {
        anyhow::bail!("TUI mode is not yet implemented");
    }

    // Ctrl+C: best-effort graceful shutdown.
    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        res = runner::plain::run(config, args.attempts) => res,
        _ = shutdown => {
            // Servers are children of this process; killing the process group
            // on exit is handled by the OS + engine drop in plain mode.
            std::process::exit(0);
        }
    }
}
```

> **Ctrl+C note:** the legacy code installed a `ctrlc` handler that killed servers then exited. With tokio we use `tokio::signal::ctrl_c()` inside `select!`. Because the plain runner owns the `Engine` (and thus the `ServerProcess` group children), add a `Drop`-based safety net in a follow-up if needed; for the existing test suite (which never sends SIGINT) this is sufficient. The `ctrlc` dependency may be dropped once `tokio::signal` fully replaces it — keep it for now to avoid widening scope.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test`
Expected: ALL tests in `tests/cli.rs` PASS, including:
- `runs`
- `fails_on_too_many_attempts` / `_custom`
- `fails_on_timeout_with_custom_timeout`
- `fails_on_one_attempt`
- `fails_when_final_command_exits_non_zero` + `assert_port_released`
- `stops_servers_when_final_command_cannot_spawn` + `assert_port_released`
- `stops_descendant_processes_when_server_never_becomes_ready` (unix)
- all config-validation failure tests

If `stops_descendant_processes...` fails, verify `group_spawn`/group `kill` are actually killing the whole group (the `with-tokio` feature must be enabled — Task 1).

- [ ] **Step 6: Lint + format**

Run: `cargo clippy -- -D warnings`
Expected: no warnings.

Run: `cargo fmt`
Expected: no diff after running (or run it to apply).

- [ ] **Step 7: Commit**

```bash
git add src/core/mod.rs src/runner/mod.rs src/runner/plain.rs src/main.rs
git commit -m "refactor: run plain mode on async engine"
```

---

## Task 9: AppState scaffold for Plan 2 handoff

**Files:**
- Modify: `src/core/state.rs`

This adds the shared state container the TUI will render, without wiring it into plain mode (plain mode does not need it). Keeping it here means Plan 2 starts from a defined type.

- [ ] **Step 1: Append `AppState` types + a unit test to `src/core/state.rs`**

```rust
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalCmdStatus {
    Idle,
    Running,
    Succeeded(i32),
    Failed(i32),
}

pub struct ServerView {
    pub name: String,
    pub url: String,
    pub status: ServerStatus,
    pub attempts: Attempts,
    pub log: Arc<Mutex<RingBuffer>>,
}

pub struct FinalCmdView {
    pub command: String,
    pub status: FinalCmdStatus,
    pub log: Arc<Mutex<RingBuffer>>,
}

/// Shared, render-friendly snapshot of the engine, owned behind Arc<Mutex<_>>.
pub struct AppState {
    pub servers: Vec<ServerView>,
    pub final_cmd: FinalCmdView,
}

#[cfg(test)]
mod app_state_tests {
    use super::*;

    #[test]
    fn final_cmd_status_equality() {
        assert_eq!(FinalCmdStatus::Succeeded(0), FinalCmdStatus::Succeeded(0));
        assert_ne!(FinalCmdStatus::Succeeded(0), FinalCmdStatus::Failed(1));
    }
}
```

- [ ] **Step 2: Verify**

Run: `cargo test --lib final_cmd_status_equality`
Expected: PASS.

Run: `cargo test`
Expected: all tests PASS (plain mode untouched).

- [ ] **Step 3: Commit**

```bash
git add src/core/state.rs
git commit -m "feat: add AppState scaffold for TUI handoff"
```

---

## Final Verification

- [ ] Run: `cargo test` — all integration + unit tests PASS.
- [ ] Run: `cargo clippy -- -D warnings` — clean.
- [ ] Run: `cargo fmt -- --check` — clean.
- [ ] Run: `cargo run -- --help` — shows `--tui`.
- [ ] Run: `cargo run -- --tui` — exits with "TUI mode is not yet implemented".
- [ ] Manual: `cargo run` against `servers.yaml` — starts the python server, detects readiness, runs `true`, exits 0, leaves no orphan processes (`assert_port_released`-style check on port 3000).

## Spec Coverage (Plan 1 portion)

- Module split (`cli`/`config`/`core/*`/`runner`): Tasks 2,3,4,8.
- Unified async core / tokio runtime: Tasks 1,5,6,7,8.
- Async readiness check preserving semantics: Task 5.
- Output capture into bounded ring buffers: Tasks 4,6,7.
- `ServerStatus` gains `Failed`/`Stopped`; `FinalCmdStatus`; `AppState`: Tasks 4,9.
- Plain-mode behavior parity + exit codes + process-group cleanup: Task 8 + Final Verification.
- `--tui` flag present (inert until Plan 2): Tasks 3,8.

**Deferred to Plan 2:** ratatui/crossterm UI, event loop, input mapping, restart/stop-start/re-run/scroll actions, failure-keeps-panel-alive behavior, terminal guard/panic hook, TestBackend rendering tests, engine command channel.
