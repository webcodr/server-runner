use std::process::{Child, Command};

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
    cmd.spawn().expect("Failed to start server")
}

fn check_server(server: Server) -> bool {
    reqwest::blocking::get(server.url)
        .expect("Could not obtain status of server.")
        .status()
        .is_success()
}

fn get_config() -> Result<Config, config::ConfigError> {
    let settings = config::Config::builder()
        .add_source(config::File::new("servers.yaml", config::FileFormat::Yaml))
        .build()
        .expect("Could not load file servers.yaml");

    settings.try_deserialize::<Config>()
}

fn main() {
    let config = get_config().expect("Could not load server config");

    for server in config.servers {
        println!(
            "Name: {}, URL: {}, Command: {}",
            server.name, server.url, server.command
        )
    }
}
