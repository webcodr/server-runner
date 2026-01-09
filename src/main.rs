use anyhow::{Context, bail};
use clap::Parser;
use log::info;
use std::collections::HashMap;
use std::ops::AddAssign;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::sync::{Arc, LockResult, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;
use std::{env, fmt};

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
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_timeout() -> u64 {
    5
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

#[derive(Copy, Clone, Debug)]
struct Attempts(u8);

impl AddAssign<u8> for Attempts {
    fn add_assign(&mut self, other: u8) {
        self.0 = self.0.wrapping_add(other);
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
struct ServerName(String);

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

    loop {
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
            let mut process =
                run_command(&command).context(format!("Could not start process {}", command))?;

            info!("Running command {}", command);

            process.wait()?;

            info!("Command {} finished successfully", command);

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

    if config.servers.is_empty() {
        bail!("Configuration must include at least one server");
    }

    if config.command.trim().is_empty() {
        bail!("Configuration must include a command to run");
    }

    Ok(config)
}

fn start_servers(servers: &Vec<Server>) -> anyhow::Result<Vec<ServerProcess>> {
    let mut server_processes = Vec::with_capacity(servers.len());

    for s in servers {
        info!("Starting server {}", s.name);

        let server_process = ServerProcess {
            name: s.name.to_string(),
            process: run_command(&s.command)?,
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
        cmd.creation_flags(0x08000000);
    }

    Ok(cmd.spawn()?)
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

    if *attempts == max_attempts {
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
