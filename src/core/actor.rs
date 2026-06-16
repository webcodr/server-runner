use tokio::sync::mpsc;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::Config;
use crate::core::server::ServerProcess;
use crate::core::state::{AppState, FinalCmdStatus, ServerStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)] // wired into the TUI in a later task
pub enum EngineCommand {
    Restart(usize),
    StopStart(usize),
    RerunFinalCommand,
    Quit,
}

#[allow(dead_code)] // wired into the TUI in a later task
pub async fn run_tui_engine(
    config: Config,
    max_attempts: u8,
    state: Arc<Mutex<AppState>>,
    mut commands: mpsc::Receiver<EngineCommand>,
) -> anyhow::Result<()> {
    let mut processes: Vec<Option<ServerProcess>> = Vec::with_capacity(config.servers.len());
    for (idx, server) in config.servers.iter().enumerate() {
        match ServerProcess::spawn_captured(&server.name, &server.command) {
            Ok(process) => {
                link_server_log(&state, idx, Arc::clone(&process.log));
                processes.push(Some(process));
            }
            Err(error) => {
                let _ = stop_all(&mut processes).await;
                return Err(error);
            }
        }
    }

    let mut final_status_started = false;
    let mut ticker = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(EngineCommand::Quit) | None => {
                        stop_all(&mut processes).await?;
                        return Ok(());
                    }
                    Some(EngineCommand::Restart(idx)) => {
                        if idx < processes.len() {
                            if let Err(error) = start_server_process(
                                &mut processes,
                                &state,
                                &config.servers[idx],
                                idx,
                                "Restarting server",
                            )
                            .await
                            {
                                stop_all(&mut processes).await?;
                                return Err(error);
                            }
                            final_status_started = false;
                        }
                    }
                    Some(EngineCommand::StopStart(idx)) => {
                        if idx < processes.len() {
                            let status = server_status(&state, idx);
                            if matches!(
                                status,
                                Some(ServerStatus::Stopped) | Some(ServerStatus::Failed)
                            ) {
                                if let Err(error) = start_server_process(
                                    &mut processes,
                                    &state,
                                    &config.servers[idx],
                                    idx,
                                    "Starting server",
                                )
                                .await
                                {
                                    stop_all(&mut processes).await?;
                                    return Err(error);
                                }
                            } else {
                                if let Some(mut process) = processes[idx].take() {
                                    let _ = process.stop().await;
                                }
                                mark_server_stopped(&state, idx);
                            }
                            final_status_started = false;
                        }
                    }
                    Some(EngineCommand::RerunFinalCommand) => {}
                }
            }
            _ = ticker.tick() => {
                if let Err(error) = poll_servers(&config, max_attempts, &state).await {
                    stop_all(&mut processes).await?;
                    return Err(error);
                }
                if !final_status_started && all_servers_running(&state) {
                    final_status_started = true;
                    if let Err(error) = run_final_command_for_tui(&config.command, &state).await {
                        stop_all(&mut processes).await?;
                        return Err(error);
                    }
                }
            }
        }
    }
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
async fn poll_servers(
    config: &Config,
    max_attempts: u8,
    state: &Arc<Mutex<AppState>>,
) -> anyhow::Result<()> {
    for (idx, server) in config.servers.iter().enumerate() {
        if server_status(state, idx) != Some(ServerStatus::Waiting) {
            continue;
        }

        let status = crate::core::health::check(&server.name, &server.url, server.timeout).await?;
        if status == ServerStatus::Running {
            mark_server_running(state, idx);
        } else {
            increment_attempt_or_fail(state, idx, max_attempts);
        }
    }

    Ok(())
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
fn mark_server_running(state: &Arc<Mutex<AppState>>, idx: usize) {
    if let Ok(mut state) = state.lock()
        && let Some(server) = state.servers.get_mut(idx)
    {
        server.status = ServerStatus::Running;
    }
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
fn reset_server_for_start(state: &Arc<Mutex<AppState>>, idx: usize, reason: &str) {
    if let Ok(mut guard) = state.lock()
        && let Some(server) = guard.servers.get_mut(idx)
    {
        server.status = ServerStatus::Waiting;
        server.attempts = crate::core::state::Attempts(0);
        if let Ok(mut log) = server.log.lock() {
            log.push(format!("--- {reason} ---"));
        }
    }
}

/// Take and stop any process in the slot, spawn a fresh captured replacement,
/// link its log into shared state, store it, and reset the server to Waiting.
#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
async fn start_server_process(
    processes: &mut [Option<ServerProcess>],
    state: &Arc<Mutex<AppState>>,
    server: &crate::config::Server,
    idx: usize,
    reason: &str,
) -> anyhow::Result<()> {
    if let Some(mut process) = processes[idx].take() {
        let _ = process.stop().await;
    }
    let process = ServerProcess::spawn_captured(&server.name, &server.command)?;
    link_server_log(state, idx, Arc::clone(&process.log));
    processes[idx] = Some(process);
    reset_server_for_start(state, idx, reason);
    Ok(())
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
fn link_server_log(
    state: &Arc<Mutex<AppState>>,
    idx: usize,
    log: Arc<Mutex<crate::core::state::RingBuffer>>,
) {
    if let Ok(mut guard) = state.lock()
        && let Some(server) = guard.servers.get_mut(idx)
    {
        server.log = log;
    }
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
fn mark_server_stopped(state: &Arc<Mutex<AppState>>, idx: usize) {
    if let Ok(mut guard) = state.lock()
        && let Some(server) = guard.servers.get_mut(idx)
    {
        server.status = ServerStatus::Stopped;
        if let Ok(mut log) = server.log.lock() {
            log.push("--- Server stopped ---".to_string());
        }
    }
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
fn increment_attempt_or_fail(state: &Arc<Mutex<AppState>>, idx: usize, max_attempts: u8) {
    if let Ok(mut state) = state.lock()
        && let Some(server) = state.servers.get_mut(idx)
    {
        server.attempts += 1;
        if server.attempts.0 >= max_attempts {
            server.status = ServerStatus::Failed;
            let word = if max_attempts == 1 {
                "attempt"
            } else {
                "attempts"
            };
            if let Ok(mut log) = server.log.lock() {
                log.push(format!(
                    "Could not connect to server {} after {} {}",
                    server.name, server.attempts, word
                ));
            }
        }
    }
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
fn all_servers_running(state: &Arc<Mutex<AppState>>) -> bool {
    state
        .lock()
        .map(|state| {
            state
                .servers
                .iter()
                .all(|server| server.status == ServerStatus::Running)
        })
        .unwrap_or(false)
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
async fn run_final_command_for_tui(
    command: &str,
    state: &Arc<Mutex<AppState>>,
) -> anyhow::Result<()> {
    set_final_status(state, FinalCmdStatus::Running);
    let final_cmd = crate::core::command::spawn_captured(command)?;
    if let Ok(mut guard) = state.lock() {
        guard.final_cmd.log = Arc::clone(&final_cmd.log);
    }

    let mut child = final_cmd.child;
    let status = child.wait().await?;
    for reader in final_cmd.readers {
        let _ = reader.await;
    }

    let code = status.code().unwrap_or(1);
    if status.success() {
        set_final_status(state, FinalCmdStatus::Succeeded(code));
    } else {
        set_final_status(state, FinalCmdStatus::Failed(code));
    }
    Ok(())
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
fn set_final_status(state: &Arc<Mutex<AppState>>, status: FinalCmdStatus) {
    if let Ok(mut state) = state.lock() {
        state.final_cmd.status = status;
    }
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
async fn stop_all(processes: &mut [Option<ServerProcess>]) -> anyhow::Result<()> {
    let mut first_error = None;

    for process in processes.iter_mut().flatten() {
        if let Err(error) = process.stop().await {
            remember_first_error(&mut first_error, error);
        }
    }

    if let Some(error) = first_error {
        return Err(error);
    }

    Ok(())
}

#[allow(dead_code)] // used through stop_all once the TUI is wired in
fn remember_first_error(first_error: &mut Option<anyhow::Error>, error: anyhow::Error) {
    if first_error.is_none() {
        *first_error = Some(error);
    }
}

#[allow(dead_code)] // used through run_tui_engine once the TUI is wired in
fn server_status(state: &Arc<Mutex<AppState>>, idx: usize) -> Option<ServerStatus> {
    state
        .lock()
        .ok()
        .and_then(|state| state.servers.get(idx).map(|server| server.status))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{mark_server_stopped, reset_server_for_start};
    use crate::config::{Config, Server};
    use crate::core::state::{AppState, ServerStatus};

    fn one_server_config() -> Config {
        Config {
            servers: vec![Server {
                name: "API".to_string(),
                url: "http://127.0.0.1:3000".to_string(),
                command: "server-command".to_string(),
                timeout: 1,
            }],
            command: "test-command".to_string(),
        }
    }

    #[test]
    fn all_running_detects_only_running_servers() {
        let config = one_server_config();
        let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

        assert!(!super::all_servers_running(&state));

        state.lock().unwrap().servers[0].status = ServerStatus::Running;

        assert!(super::all_servers_running(&state));
    }

    #[test]
    fn marks_failed_when_attempts_exhausted() {
        let config = one_server_config();
        let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

        super::increment_attempt_or_fail(&state, 0, 1);

        let state = state.lock().unwrap();
        assert_eq!(state.servers[0].attempts, 1u8);
        assert_eq!(state.servers[0].status, ServerStatus::Failed);
    }

    #[test]
    fn mark_server_running_ignores_missing_state_index() {
        let state = Arc::new(Mutex::new(AppState::new(&[], "test-command")));

        super::mark_server_running(&state, 0);

        assert!(state.lock().unwrap().servers.is_empty());
    }

    #[test]
    fn remember_first_error_preserves_original_error() {
        let mut first = None;

        super::remember_first_error(&mut first, anyhow::anyhow!("first"));
        super::remember_first_error(&mut first, anyhow::anyhow!("second"));

        assert_eq!(first.unwrap().to_string(), "first");
    }

    #[test]
    fn restart_resets_server_state() {
        let config = crate::config::Config {
            servers: vec![crate::config::Server {
                name: "A".to_string(),
                url: "http://127.0.0.1:1".to_string(),
                command: "echo a".to_string(),
                timeout: 1,
            }],
            command: "echo done".to_string(),
        };
        let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));
        {
            let mut guard = state.lock().unwrap();
            guard.servers[0].status = ServerStatus::Failed;
            guard.servers[0].attempts = crate::core::state::Attempts(5);
        }

        reset_server_for_start(&state, 0, "Restarting server");

        let guard = state.lock().unwrap();
        assert_eq!(guard.servers[0].status, ServerStatus::Waiting);
        assert_eq!(guard.servers[0].attempts, 0u8);
        let lines: Vec<_> = guard.servers[0]
            .log
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect();
        assert!(lines.contains(&"--- Restarting server ---".to_string()));
    }

    #[test]
    fn stop_marks_server_stopped() {
        let config = crate::config::Config {
            servers: vec![crate::config::Server {
                name: "A".to_string(),
                url: "http://127.0.0.1:1".to_string(),
                command: "echo a".to_string(),
                timeout: 1,
            }],
            command: "echo done".to_string(),
        };
        let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

        mark_server_stopped(&state, 0);

        assert_eq!(
            state.lock().unwrap().servers[0].status,
            ServerStatus::Stopped
        );
    }
}

#[cfg(all(test, unix))]
mod final_command_tests {
    use std::sync::{Arc, Mutex};

    use super::run_final_command_for_tui;
    use crate::core::state::{AppState, FinalCmdStatus};

    #[tokio::test]
    async fn run_final_command_updates_success_status_and_log() {
        let config = crate::config::Config {
            servers: Vec::new(),
            command: "sh -c 'echo final-ok'".to_string(),
        };
        let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

        run_final_command_for_tui(&config.command, &state)
            .await
            .unwrap();

        let guard = state.lock().unwrap();
        assert_eq!(guard.final_cmd.status, FinalCmdStatus::Succeeded(0));
        let lines: Vec<_> = guard
            .final_cmd
            .log
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect();
        assert!(lines.contains(&"final-ok".to_string()));
    }

    #[tokio::test]
    async fn run_final_command_updates_failed_status() {
        let config = crate::config::Config {
            servers: Vec::new(),
            command: "sh -c 'exit 7'".to_string(),
        };
        let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

        run_final_command_for_tui(&config.command, &state)
            .await
            .unwrap();

        assert_eq!(
            state.lock().unwrap().final_cmd.status,
            FinalCmdStatus::Failed(7)
        );
    }
}

#[cfg(all(test, unix))]
mod command_path_tests {
    use std::sync::{Arc, Mutex};

    use super::start_server_process;
    use crate::core::server::ServerProcess;
    use crate::core::state::{AppState, ServerStatus};

    fn pid_alive(pid: u32) -> bool {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn starting_failed_server_stops_old_process() {
        let config = crate::config::Config {
            servers: vec![crate::config::Server {
                name: "A".to_string(),
                url: "http://127.0.0.1:1".to_string(),
                command: "sh -c 'sleep 5'".to_string(),
                timeout: 1,
            }],
            command: "echo done".to_string(),
        };
        let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

        // A Failed server still owns a live process in its slot.
        let old = ServerProcess::spawn_captured("A", "sh -c 'sleep 5'").unwrap();
        let old_pid = old.id().expect("old process should expose a pid");
        let mut processes: Vec<Option<ServerProcess>> = vec![Some(old)];
        state.lock().unwrap().servers[0].status = ServerStatus::Failed;

        assert!(
            pid_alive(old_pid),
            "old process should be alive before respawn"
        );

        start_server_process(
            &mut processes,
            &state,
            &config.servers[0],
            0,
            "Starting server",
        )
        .await
        .unwrap();

        // Give the OS a moment to reap the killed group.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        assert!(
            !pid_alive(old_pid),
            "old process must be stopped, not leaked"
        );

        let new_pid = processes[0]
            .as_ref()
            .expect("a fresh process should occupy the slot")
            .id();
        assert!(new_pid.is_some());
        assert_ne!(Some(old_pid), new_pid, "slot should hold a new process");
        assert_eq!(
            state.lock().unwrap().servers[0].status,
            ServerStatus::Waiting
        );

        // Clean up the replacement so the test leaves no lingering process.
        if let Some(mut process) = processes[0].take() {
            let _ = process.stop().await;
        }
    }
}
