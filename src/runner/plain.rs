use anyhow::Context;
use tokio::sync::watch;

use std::time::Duration;

use crate::config::{Config, Server};
use crate::core::Engine;
use crate::core::state::ServerStatus;

/// Drive the engine in plain-log mode.
pub async fn run(config: Config, max_attempts: u8) -> anyhow::Result<()> {
    // Registered before the servers start, so a Ctrl+C arriving during startup
    // is already observable.
    let mut shutdown = shutdown_signal();

    let Config { servers, command } = config;
    let mut engine = Engine::start(&servers, max_attempts)?;
    let mut ready = vec![false; servers.len()];

    loop {
        // `None` means Ctrl+C won the race and the probe round was cancelled.
        let round = tokio::select! {
            result = probe_pending(&mut engine, &servers, &mut ready) => Some(result),
            _ = shutdown.changed() => None,
        };

        match round {
            None => return stop_for_shutdown(&mut engine).await,
            Some(Err(error)) => {
                engine.stop_all().await?;
                return Err(error);
            }
            Some(Ok(true)) => {
                let final_result = run_final_command(&command, &mut shutdown).await;
                engine.stop_all().await?;
                return final_result;
            }
            Some(Ok(false)) => {}
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            _ = shutdown.changed() => return stop_for_shutdown(&mut engine).await,
        }
    }
}

/// Watch channel that flips to `true` on the first Ctrl+C.
///
/// A dedicated task owns one long-lived `ctrl_c()` future. Building a fresh one
/// per `select!` iteration would drop any signal delivered while that future was
/// not being polled — during a health probe, for instance — and because tokio
/// installs a handler the default terminate action is gone too, so the signal
/// would be lost entirely rather than killing the process.
fn shutdown_signal() -> watch::Receiver<bool> {
    let (tx, rx) = watch::channel(false);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = tx.send(true);
        }
    });
    rx
}

/// Probe every server that is not ready yet and report whether all are up.
///
/// Servers already known to be ready are skipped: re-probing them keeps
/// incrementing their attempt counter, so a healthy server could exhaust its
/// attempts — and get blamed in the error — while a different, slower server
/// was the one actually holding up the run.
async fn probe_pending(
    engine: &mut Engine,
    servers: &[Server],
    ready: &mut [bool],
) -> anyhow::Result<bool> {
    for (index, server) in servers.iter().enumerate() {
        if ready[index] {
            continue;
        }

        if engine.probe(server).await? == ServerStatus::Running {
            ready[index] = true;
        }
    }

    Ok(ready.iter().all(|ready| *ready))
}

/// Ctrl+C path: stop every server, then exit successfully.
async fn stop_for_shutdown(engine: &mut Engine) -> anyhow::Result<()> {
    engine.stop_all().await.context("Error stopping servers")
}

async fn run_final_command(
    command: &str,
    shutdown: &mut watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut final_cmd = crate::core::command::spawn_group(command, true)
        .context(format!("Could not start process {}", command))?;

    log::info!("Running command {}", command);

    let status = tokio::select! {
        status = final_cmd.wait() => status?,
        _ = shutdown.changed() => {
            // Kill the command and everything it spawned. Exiting the process
            // here instead would leave the whole group orphaned.
            let _ = final_cmd.cancel().await;
            for reader in final_cmd.readers {
                let _ = reader.await;
            }
            return Ok(());
        }
    };

    for reader in final_cmd.readers {
        let _ = reader.await;
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
