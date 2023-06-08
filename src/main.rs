use anyhow::{bail, Context};
use clap::Parser;
use std::env;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

#[derive(Parser)]
struct Args {
    #[arg(short, long, default_value = "servers.yaml")]
    config: String,
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
    WAITING,
    RUNNING,
}

fn run_command(command: &String) -> anyhow::Result<Child> {
    let command_parts: Vec<&str> = command.split(" ").collect();
    let mut cmd = Command::new(command_parts[0]);

    for i in 1..command_parts.len() {
        cmd.arg(command_parts[i]);
    }

    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }

    let child = cmd
        .spawn()
        .context(format!("Could not start procces '{}'", &command))?;

    Ok(child)
}

fn check_server(server: &Server) -> anyhow::Result<ServerStatus> {
    println!("Checking server {} on url {}", &server.name, &server.url);

    let result = match reqwest::blocking::get(&server.url) {
        Ok(response) => response.status(),
        Err(error) => {
            if error.is_connect() {
                return Ok(ServerStatus::WAITING);
            } else {
                bail!(
                    "Could not connect to server {} on url {}",
                    &server.name,
                    &server.url
                );
            }
        }
    };

    return if result.is_success() {
        Ok(ServerStatus::RUNNING)
    } else {
        Ok(ServerStatus::WAITING)
    };
}

fn get_config(filename: &String) -> anyhow::Result<Config> {
    let cwd = env::current_dir()?;
    let config_file_path = cwd.join(&filename);
    let settings = config::Config::builder()
        .add_source(config::File::new(
            config_file_path
                .to_str()
                .context("Could not convert file path to string")?,
            config::FileFormat::Yaml,
        ))
        .build()
        .context(format!("Could not find config file {}", &filename))?;

    let config = settings
        .try_deserialize::<Config>()
        .context(format!("Could not parse config file {}", &filename))?;

    Ok(config)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = get_config(&args.config)?;
    let mut server_processes = Vec::with_capacity(config.servers.len());

    println!("Running on {}", env::consts::OS);
    println!(
        "Current working directory: {}",
        env::current_dir()?.display()
    );

    for server in &config.servers {
        println!("Starting server {}", server.name);

        let process = run_command(&server.command)?;
        let server_process = ServerProcess {
            name: server.name.to_string(),
            process,
        };

        server_processes.push(server_process);
    }

    loop {
        let mut ready = true;

        for server in &config.servers {
            if check_server(&server)? == ServerStatus::WAITING {
                println!("Server {} not ready, waiting 1 s", &server.name);
                ready = false;
            }
        }

        if ready == true {
            let mut process = run_command(&config.command)
                .context(format!("Could not start process {}", &config.command))?;

            println!("Running command {}", &config.command);

            process.wait()?;

            println!("Command {} finished successfully", &config.command);

            break;
        } else {
            thread::sleep(Duration::from_secs(1));
        }
    }

    for mut server_process in server_processes {
        println!("Stopping server {}", server_process.name);

        server_process.process.kill().context(format!(
            "Failed to stop server process {}",
            server_process.name
        ))?;
    }

    Ok(())
}
