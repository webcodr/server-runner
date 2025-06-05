use anyhow::{bail, Context};
use clap::Parser;
use log::info;
use std::collections::HashMap;
use std::env;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::sync::{Arc, LockResult, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(short, long, default_value = "servers.yaml")]
    config: String,

    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    #[arg(short, long, default_value_t = 10)]
    attempts: u8,
}

#[derive(serde::Deserialize)]
struct Server {
    name: String,
    url: String,
    command: String,
}

#[derive(serde::Deserialize)]
struct Config {
    servers: Vec<Server>,
    command: String,
}

struct ServerProcess {
    name: String,
    process: Child,
}

#[derive(PartialEq, Eq)]
enum ServerStatus {
    Waiting,
    Running,
}

fn run(args: Args) -> anyhow::Result<()> {
    let config = get_config(&args.config)?;
    let server_processes = Arc::new(Mutex::new(start_servers(&config)?));
    let mut attempts: HashMap<String, u8> = HashMap::new();
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

    let server_processes_clone = Arc::clone(&server_processes);

    ctrlc::set_handler(move || {
        let mut processes = server_processes_clone.lock();

        stop_servers_and_log(&mut processes);

        std::process::exit(0);
    })?;

    loop {
        let mut ready = true;

        for server in &config.servers {
            match check_server(server, &mut attempts, args.attempts) {
                Ok(result) => {
                    if result == ServerStatus::Waiting {
                        ready = false;
                    }
                }
                Err(e) => {
                    stop_servers_and_log(&mut server_processes.lock());

                    return Err(e);
                }
            }
        }

        if ready {
            let mut process = run_command(&config.command)
                .context(format!("Could not start process {}", &config.command))?;

            info!("Running command {}", &config.command);

            process.wait()?;

            info!("Command {} finished successfully", &config.command);

            break;
        }

        thread::sleep(Duration::from_secs(1));
    }

    stop_servers_and_log(&mut server_processes.lock());

    Ok(())
}

fn get_config(filename: &str) -> anyhow::Result<Config> {
    let cwd = env::current_dir()?;
    let tmp_path = cwd.join(filename);
    let config_file_path = tmp_path.to_str().context(format!(
        "Could not create String from Path {}",
        tmp_path.display()
    ))?;

    info!("Loading config file {}", config_file_path);

    let settings = config::Config::builder()
        .add_source(config::File::new(
            config_file_path,
            config::FileFormat::Yaml,
        ))
        .build()
        .context(format!("Could not find config file {}", filename))?;

    let config = settings
        .try_deserialize::<Config>()
        .context(format!("Could not parse config file {}", filename))?;

    Ok(config)
}

fn start_servers(config: &Config) -> anyhow::Result<Vec<ServerProcess>> {
    let mut server_processes = Vec::with_capacity(config.servers.len());

    for s in &config.servers {
        info!("Starting server {}", s.name);

        let process = run_command(&s.command)?;

        let server_process = ServerProcess {
            name: s.name.to_string(),
            process,
        };

        server_processes.push(server_process);
    }

    Ok(server_processes)
}

fn stop_servers(server_processes: &mut [ServerProcess]) -> anyhow::Result<()> {
    for p in server_processes.iter_mut() {
        info!("Stopping server {}", p.name);

        p.process
            .kill()
            .context(format!("Failed to stop process {}", p.name))?;
    }

    Ok(())
}

fn stop_servers_and_log(server_processes: &mut LockResult<MutexGuard<Vec<ServerProcess>>>) {
    let processes = match server_processes {
        Ok(p) => p,
        Err(e) => {
            info!("Could not stop servers: {}", e);

            std::process::exit(0);
        }
    };

    match stop_servers(processes) {
        Ok(_) => info!("All servers stopped successfully"),
        Err(e) => info!("Could not stop servers: {}", e),
    }
}

fn run_command(command: &str) -> anyhow::Result<Child> {
    let command_parts: Vec<&str> = command.split(' ').collect();

    if command_parts.is_empty() {
        bail!("Empty command provided");
    }

    let mut cmd = Command::new(command_parts[0]);

    for part in command_parts.iter().skip(1) {
        cmd.arg(part);
    }

    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }

    let child = cmd
        .spawn()
        .context(format!("Could not start process '{}'", command))?;

    Ok(child)
}

fn check_server(
    server: &Server,
    server_attempts: &mut HashMap<String, u8>,
    max_attempts: u8,
) -> anyhow::Result<ServerStatus> {
    let server_name = &server.name;

    let attempts = server_attempts
        .entry(server_name.to_owned())
        .and_modify(|attempts| *attempts += 1)
        .or_insert(1);

    if *attempts == max_attempts {
        bail!(
            "Could not connect to server {} after {} attempts",
            server_name,
            attempts
        );
    }

    info!(
        "Checking server {} on url {}, attempt {}, waiting one second ...",
        server_name, &server.url, attempts
    );

    let result = match reqwest::blocking::get(&server.url) {
        Ok(response) => response.status(),
        Err(error) => {
            if error.is_connect() {
                return Ok(ServerStatus::Waiting);
            } else {
                bail!(
                    "Could not connect to server {} on url {}",
                    server_name,
                    &server.url
                );
            }
        }
    };

    if result.is_success() {
        Ok(ServerStatus::Running)
    } else {
        Ok(ServerStatus::Waiting)
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    run(args)
}
