use anyhow::bail;
use log::info;
use std::collections::HashMap;
use std::process::{Child, Output};
use std::sync::{LockResult, MutexGuard};
use std::thread;
use std::time::Duration;

use crate::{
    attempts::Attempts,
    command::{spawn_command, execute_command as execute_cmd},
    config::Server,
    constants::HEALTH_CHECK_INTERVAL_SECONDS,
};

pub struct ServerProcess {
    pub name: String,
    pub process: Child,
    pub stdout_reader: Option<std::process::ChildStdout>,
    pub stderr_reader: Option<std::process::ChildStderr>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum ServerStatus {
    Waiting,
    Running,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServerName(pub String);

pub fn start_servers(servers: &Vec<Server>, enable_logging: bool) -> anyhow::Result<Vec<ServerProcess>> {
    let mut server_processes = Vec::with_capacity(servers.len());

    for s in servers {
        if enable_logging {
            info!("Starting server {}", s.name);
        }

        let mut process = spawn_command(&s.command)?;
        let stdout_reader = process.stdout.take();
        let stderr_reader = process.stderr.take();
        
        let server_process = ServerProcess {
            name: s.name.to_string(),
            process,
            stdout_reader,
            stderr_reader,
        };

        server_processes.push(server_process);
    }

    Ok(server_processes)
}

pub fn cleanup_processes(processes: &mut [ServerProcess], enable_logging: bool) -> anyhow::Result<()> {
    for p in processes {
        if enable_logging {
            info!("Stopping server {}", p.name);
        }
        
        if let Err(e) = p.process.kill() {
            if enable_logging {
                info!("Failed to kill process {}: {}", p.name, e);
            }
        } else {
            let _ = p.process.wait();
            if enable_logging {
                info!("Successfully stopped server {}", p.name);
            }
        }
    }
    
    if enable_logging {
        info!("All servers cleanup completed");
    }
    Ok(())
}

pub fn stop_servers(
    server_processes: &mut LockResult<MutexGuard<Vec<ServerProcess>>>,
) -> anyhow::Result<()> {
    let processes = match server_processes {
        Ok(p) => p,
        Err(e) => bail!("{}", e),
    };

    for p in processes.iter_mut() {
        info!("Stopping server {}", p.name);

        if p.process.kill().is_ok() {
            let _ = p.process.wait();
        } else {
            bail!("Failed to stop process {}", p.name);
        }
    }

    info!("All servers stopped successfully");

    Ok(())
}

pub fn execute_command(command: &str) -> anyhow::Result<Output> {
    execute_cmd(command)
}

pub fn wait_for_servers(servers: &Vec<Server>, max_attempts: Attempts, enable_logging: bool) -> anyhow::Result<()> {
    let mut attempts = HashMap::<ServerName, Attempts>::new();

    loop {
        let mut ready = true;

        for server in servers {
            match check_server(server, &mut attempts, max_attempts.value(), enable_logging) {
                Ok(result) => {
                    if result == ServerStatus::Waiting {
                        ready = false;
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        if ready {
            break;
        }

        thread::sleep(Duration::from_secs(HEALTH_CHECK_INTERVAL_SECONDS));
    }

    Ok(())
}

fn check_server(
    server: &Server,
    server_attempts: &mut HashMap<ServerName, Attempts>,
    max_attempts: u8,
    enable_logging: bool,
) -> anyhow::Result<ServerStatus> {
    let Server { name, url, timeout, .. } = server;

    let attempts = server_attempts
        .entry(ServerName(name.to_owned()))
        .and_modify(|attempts| *attempts += 1)
        .or_insert(Attempts::new(1));

    if *attempts == max_attempts {
        bail!(
            "Could not connect to server {} after {} attempts",
            name,
            attempts
        );
    }

    if enable_logging {
        info!(
            "Checking server {} on url {}, attempt {}, waiting one second ...",
            name, url, attempts
        );
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(*timeout))
        .build()?;

    let result = match client.get(url).send() {
        Ok(response) => response.status(),
        Err(error) => {
            if error.is_connect() {
                return Ok(ServerStatus::Waiting);
            } else {
                bail!("Could not connect to server {} on url {}", name, url);
            }
        }
    };

    if result.is_success() {
        Ok(ServerStatus::Running)
    } else {
        Ok(ServerStatus::Waiting)
    }
}