use anyhow::{bail, Context};
use clap::Parser;
use log::info;
use std::collections::HashMap;
use std::env;
use std::error::Error;
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
    let server_processes = start_servers(&config)?;
    let server_processes_arc_mutex = Arc::new(Mutex::new(server_processes));
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

    let server_processes_clone = Arc::clone(&server_processes_arc_mutex);

    ctrlc::set_handler(move || {
        let mut processes = server_processes_clone.lock();

        match stop_servers(&mut processes) {
            Ok(_) => {}
            Err(e) => exit_with_error(Box::from(e)),
        };

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
                    stop_servers(&mut server_processes_arc_mutex.lock())?;

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

    stop_servers(&mut server_processes_arc_mutex.lock())?;

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

fn stop_servers(
    server_processes: &mut LockResult<MutexGuard<Vec<ServerProcess>>>,
) -> anyhow::Result<()> {
    let processes = match server_processes {
        Ok(p) => p,
        Err(e) => bail!("{}", e),
    };

    for p in processes.iter_mut() {
        info!("Stopping server {}", p.name);

        if p.process.kill().is_err() {
            bail!("Failed to stop process {}", p.name);
        }
    }

    info!("All servers stopped successfully");

    Ok(())
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

fn exit_with_error(e: Box<dyn Error>) {
    eprintln!("An error occurred: {}", e);

    std::process::exit(1)
}

fn main() {
    let args = Args::parse();

    match run(args) {
        Ok(_) => {}
        Err(e) => exit_with_error(Box::from(e)),
    }
}
