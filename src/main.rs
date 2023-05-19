use std::process::{Child, Command};

use clap::Parser;

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
}

fn start_server(server: Server) -> Child {
    let mut cmd = Command::new("sh");

    cmd.arg("-o").arg(server.command);
    cmd.spawn().expect(&format!("Failed {}", server.name))
}

fn check_server(server: Server) -> bool {
    reqwest::blocking::get(server.url)
        .expect(&format!(
            "Could not obtain status of server {}",
            server.name
        ))
        .status()
        .is_success()
}

fn get_config(config_file: String) -> Result<Config, config::ConfigError> {
    let settings = config::Config::builder()
        .add_source(config::File::new(&config_file, config::FileFormat::Yaml))
        .build()
        .expect(&format!(
            "Could not find configuration file {}",
            config_file
        ));

    settings.try_deserialize::<Config>()
}

fn main() {
    let args = Args::parse();
    let config = get_config(args.config).expect("Could not load server config");

    for server in config.servers {
        println!(
            "Name: {}, URL: {}, Command: {}",
            server.name, server.url, server.command
        )
    }
}
