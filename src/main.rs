use anyhow::{bail, Context};
use clap::Parser;
use std::collections::HashMap;
use std::env;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(short, long, default_value = "servers.yaml")]
    config: String,

    #[arg(short, long, default_value_t = false)]
    verbose: bool,
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

struct ServerRunner {
    args: Args,
    attempts: HashMap<String, u8>,
}

impl ServerRunner {
    fn run(&mut self) -> anyhow::Result<()> {
        let config = self.get_config(&self.args.config)?;
        let server_processes = self.start_servers(&config)?;

        loop {
            let mut ready = true;

            for server in &config.servers {
                match self.check_server(&server) {
                    Ok(result) => {
                        if result == ServerStatus::WAITING {
                            ready = false;
                        }
                    }
                    Err(e) => {
                        self.stop_servers(server_processes)?;
                        return Err(e);
                    }
                }
            }

            if ready == true {
                let mut process = self
                    .run_command(&config.command)
                    .context(format!("Could not start process {}", &config.command))?;

                self.log(format!("Running command {}", &config.command));

                process.wait()?;

                self.log(format!("Command {} finished successfully", &config.command));

                break;
            } else {
                thread::sleep(Duration::from_secs(1));
            }
        }

        self.stop_servers(server_processes)?;

        println!("{:?}", self.attempts);

        Ok(())
    }

    fn log(&self, message: String) {
        if self.args.verbose {
            println!("{}", &message);
        }
    }

    fn get_config(&self, filename: &String) -> anyhow::Result<Config> {
        let cwd = env::current_dir()?;
        let tmp_path = cwd.join(&filename);
        let config_file_path = tmp_path.to_str().context(format!(
            "Could not create String from Path {}",
            tmp_path.display()
        ))?;

        self.log(format!("Loading config file {}", config_file_path));

        let settings = config::Config::builder()
            .add_source(config::File::new(
                config_file_path,
                config::FileFormat::Yaml,
            ))
            .build()
            .context(format!("Could not find config file {}", &filename))?;

        let config = settings
            .try_deserialize::<Config>()
            .context(format!("Could not parse config file {}", &filename))?;

        Ok(config)
    }

    fn start_servers(&self, config: &Config) -> anyhow::Result<Vec<ServerProcess>> {
        let mut server_processes = Vec::with_capacity(config.servers.len());

        for s in &config.servers {
            self.log(format!("Starting server {}", s.name));

            let process = self.run_command(&s.command)?;

            let server_process = ServerProcess {
                name: s.name.to_string(),
                process,
            };

            server_processes.push(server_process);
        }

        Ok(server_processes)
    }

    fn stop_servers(&self, processes: Vec<ServerProcess>) -> anyhow::Result<()> {
        for mut p in processes {
            self.log(format!("Stopping server {}", p.name));

            p.process
                .kill()
                .context(format!("Failed to stop process {}", p.name))?;
        }

        Ok(())
    }

    fn run_command(&self, command: &String) -> anyhow::Result<Child> {
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

    fn check_server(&mut self, server: &Server) -> anyhow::Result<ServerStatus> {
        let server_name = &server.name;

        if self.attempts.contains_key(server_name) {
            *self.attempts.get_mut(server_name).unwrap() += 1;
        } else {
            self.attempts.insert(server_name.to_owned(), 1);
        }

        let attempts = *self.attempts.get(server_name).unwrap();

        if attempts == 10 {
            bail!(
                "Could not connect to server {} after {} attempts",
                server_name,
                attempts
            );
        }

        self.log(format!(
            "Checking server {} on url {}, attempt {}, waiting one second ...",
            server_name, &server.url, attempts
        ));

        let result = match reqwest::blocking::get(&server.url) {
            Ok(response) => response.status(),
            Err(error) => {
                if error.is_connect() {
                    return Ok(ServerStatus::WAITING);
                } else {
                    bail!(
                        "Could not connect to server {} on url {}",
                        &server_name,
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
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let attempts: HashMap<String, u8> = HashMap::new();

    ServerRunner { args, attempts }.run()?;

    Ok(())
}
