#[derive(Debug, serde::Deserialize)]
struct Server {
    name: String,
    port: u16,
    command: String,
}

#[derive(Debug, serde::Deserialize)]
struct Config {
    servers: Vec<Server>,
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
            "Name: {}, Port: {}, Command: {}",
            server.name, server.port, server.command
        )
    }
}
