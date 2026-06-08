use anyhow::{Context, bail};
use clap::Parser;
use command_group::{CommandGroup, GroupChild};
use log::info;
use std::collections::HashMap;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::sync::{Arc, LockResult, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

mod cli;
mod config;
mod core;
use cli::Args;
use config::{Config, Server, get_config};
use core::state::{Attempts, ServerName, ServerStatus};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

struct ServerProcess {
    name: String,
    process: GroupChild,
}

fn run(args: Args) -> anyhow::Result<()> {
    let Config { servers, command } = get_config(&args.config)?;
    let server_processes = start_servers(&servers)?;
    let server_processes_arc_mutex = Arc::new(Mutex::new(server_processes));
    let server_processes_clone = Arc::clone(&server_processes_arc_mutex);
    let mut attempts = HashMap::<ServerName, Attempts>::new();
    let log_level = if args.verbose {
        simplelog::LevelFilter::Info
    } else {
        simplelog::LevelFilter::Warn
    };

    simplelog::TermLogger::init(
        log_level,
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    )?;

    ctrlc::set_handler(move || {
        let mut processes = server_processes_clone.lock();

        match stop_servers(&mut processes) {
            Ok(_) => info!("All servers stopped successfully"),
            Err(e) => {
                eprintln!("Error stopping servers: {}", e);
                std::process::exit(1);
            }
        };

        std::process::exit(0);
    })?;

    let final_command_result = loop {
        let mut ready = true;

        for server in &servers {
            match check_server(server, &mut attempts, args.attempts) {
                Ok(result) => {
                    if result == ServerStatus::Waiting {
                        ready = false;
                    }
                }
                Err(e) => {
                    stop_servers(&mut server_processes_arc_mutex.lock())?;

                    return Err(e);
                }
            }
        }

        if ready {
            let final_command_result = match run_command(&command)
                .context(format!("Could not start process {}", command))
            {
                Ok(mut process) => {
                    info!("Running command {}", command);

                    match process.wait() {
                        Ok(status) if status.success() => {
                            info!("Command {} finished successfully", command);

                            Ok(())
                        }
                        Ok(status) => Err(anyhow::anyhow!(
                            "Command {} failed with exit status {}",
                            command,
                            status
                        )),
                        Err(error) => Err(error.into()),
                    }
                }
                Err(error) => Err(error),
            };

            break final_command_result;
        }

        thread::sleep(Duration::from_secs(1));
    };

    stop_servers(&mut server_processes_arc_mutex.lock())?;

    final_command_result
}

fn start_servers(servers: &Vec<Server>) -> anyhow::Result<Vec<ServerProcess>> {
    let mut server_processes = Vec::with_capacity(servers.len());

    for s in servers {
        info!("Starting server {}", s.name);

        let server_process = ServerProcess {
            name: s.name.to_string(),
            process: run_server_command(&s.command)?,
        };

        server_processes.push(server_process);
    }

    Ok(server_processes)
}

fn stop_servers(
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

fn run_command(command: &str) -> anyhow::Result<Child> {
    let mut cmd = build_command(command)?;

    Ok(cmd.spawn()?)
}

fn run_server_command(command: &str) -> anyhow::Result<GroupChild> {
    let mut cmd = build_command(command)?;

    #[cfg(windows)]
    {
        Ok(cmd.group().creation_flags(CREATE_NO_WINDOW).spawn()?)
    }

    #[cfg(not(windows))]
    {
        Ok(cmd.group_spawn()?)
    }
}

fn build_command(command: &str) -> anyhow::Result<Command> {
    let command_parts =
        shlex::split(command).ok_or_else(|| anyhow::anyhow!("Invalid command: {}", command))?;

    if command_parts.is_empty() {
        bail!("Empty command provided");
    }

    let mut cmd = Command::new(&command_parts[0]);

    for part in command_parts.iter().skip(1) {
        cmd.arg(part);
    }

    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    Ok(cmd)
}

fn check_server(
    server: &Server,
    server_attempts: &mut HashMap<ServerName, Attempts>,
    max_attempts: u8,
) -> anyhow::Result<ServerStatus> {
    let Server {
        name, url, timeout, ..
    } = server;

    let attempts = server_attempts
        .entry(ServerName(name.to_owned()))
        .and_modify(|attempts| *attempts += 1)
        .or_insert(Attempts(1));

    if attempts.0 >= max_attempts {
        let attempt_word = if max_attempts == 1 {
            "attempt"
        } else {
            "attempts"
        };
        bail!(
            "Could not connect to server {} after {} {}",
            name,
            attempts,
            attempt_word
        );
    }

    info!(
        "Checking server {} on url {}, attempt {}, waiting one second ...",
        name, url, attempts
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(*timeout))
        .redirect(reqwest::redirect::Policy::none())
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

fn exit_with_error(e: anyhow::Error) -> ! {
    eprintln!("An error occurred: {}", e);

    std::process::exit(1)
}

fn main() {
    let args = Args::parse();

    match run(args) {
        Ok(_) => {}
        Err(e) => exit_with_error(e),
    }
}
