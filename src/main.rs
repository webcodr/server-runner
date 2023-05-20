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

fn run_command(command: &String) -> Result<Child, std::io::Error> {
    let command_parts: Vec<&str> = command.split(" ").collect();
    let mut cmd = Command::new(command_parts[0]);

    for i in 1..command_parts.len() {
        cmd.arg(command_parts[i]);
    }

    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }
    cmd.spawn()
}

fn check_server(server: &Server) -> bool {
    println!("Checking server {} on url {}", &server.name, &server.url);

    let result = match reqwest::blocking::get(&server.url) {
        Ok(response) => response.status(),
        Err(error) => {
            if error.is_connect() {
                return false;
            } else {
                panic!("Could not connect to server")
            }
        }
    };

    return result.is_success();
}

fn get_config(filename: &String) -> Result<Config, config::ConfigError> {
    let cwd = env::current_dir().unwrap();
    let config_file_path = cwd.join(&filename);
    let settings = config::Config::builder()
        .add_source(config::File::new(
            config_file_path.to_str().unwrap(),
            config::FileFormat::Yaml,
        ))
        .build()
        .expect(&format!(
            "Could not find configuration file {}",
            &config_file_path.to_str().unwrap()
        ));

    settings.try_deserialize::<Config>()
}

fn main() {
    let args = Args::parse();
    let config = get_config(&args.config).expect("Could not load server config");
    let mut server_processes = Vec::with_capacity(config.servers.len());

    println!("Running on {}", env::consts::OS);
    println!(
        "Current working directory: {}",
        env::current_dir().unwrap().display()
    );

    for server in &config.servers {
        println!("Starting server {}", server.name);
        let process = match run_command(&server.command) {
            Ok(child) => child,
            Err(_) => panic!("Could not start server"),
        };

        let server_process = ServerProcess {
            name: server.name.to_string(),
            process,
        };

        server_processes.push(server_process);
    }

    loop {
        let mut ready = true;

        for server in &config.servers {
            if check_server(&server) == false {
                println!("Server {} not ready, waiting 1 s", &server.name);
                ready = false;
            }
        }

        if ready == true {
            let mut process = match run_command(&config.command) {
                Ok(child) => child,
                Err(_) => panic!("Could not execute command"),
            };

            println!("Running command {}", &config.command);

            process.wait().unwrap();

            println!("Command {} finished successfully", &config.command);

            break;
        } else {
            thread::sleep(Duration::from_secs(1));
        }
    }

    for mut server_process in server_processes {
        println!("Stopping server {}", server_process.name);
        server_process
            .process
            .kill()
            .expect("Failed to stop server process");
    }
}
