use anyhow::Context;

use std::time::Duration;

use crate::config::Config;
use crate::core::Engine;
use crate::core::state::ServerStatus;

/// Drive the engine in plain-log mode.
/// Exit semantics match the legacy tool exactly.
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
                    engine.stop_all().await?;
                    return Err(e);
                }
            }
        }

        if ready {
            break tokio::select! {
                result = run_final_command(&command) => result,
                _ = tokio::signal::ctrl_c() => {
                    if let Err(e) = engine.stop_all().await {
                        eprintln!("Error stopping servers: {}", e);
                        std::process::exit(1);
                    }
                    std::process::exit(0);
                }
            };
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            _ = tokio::signal::ctrl_c() => {
                if let Err(e) = engine.stop_all().await {
                    eprintln!("Error stopping servers: {}", e);
                    std::process::exit(1);
                }
                std::process::exit(0);
            }
        }
    };

    engine.stop_all().await?;
    final_result
}

async fn run_final_command(command: &str) -> anyhow::Result<()> {
    let mut final_cmd = crate::core::command::spawn(command)
        .context(format!("Could not start process {}", command))?;

    log::info!("Running command {}", command);

    // Wait for the process to finish.
    let status = final_cmd.child.wait().await?;

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
