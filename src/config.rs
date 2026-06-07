use anyhow::{Context, bail};

const MIN_TIMEOUT_SECONDS: u64 = 1;
const MAX_TIMEOUT_SECONDS: u64 = 300;

#[derive(serde::Deserialize)]
pub struct Server {
    pub name: String,
    pub url: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_timeout() -> u64 {
    5
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub servers: Vec<Server>,
    pub command: String,
}

fn validate_readiness_url(server_name: &str, url: &str) -> anyhow::Result<()> {
    let parsed = reqwest::Url::parse(url)
        .with_context(|| format!("Readiness URL for server {} is invalid", server_name))?;

    match parsed.scheme() {
        "http" | "https" => Ok(()),
        _ => bail!(
            "Readiness URL for server {} must use http or https",
            server_name
        ),
    }
}

fn validate_server_timeout(server_name: &str, timeout: u64) -> anyhow::Result<()> {
    if !(MIN_TIMEOUT_SECONDS..=MAX_TIMEOUT_SECONDS).contains(&timeout) {
        bail!(
            "Timeout for server {} must be between {} and {} seconds",
            server_name,
            MIN_TIMEOUT_SECONDS,
            MAX_TIMEOUT_SECONDS
        );
    }

    Ok(())
}

pub fn get_config(filename: &str) -> anyhow::Result<Config> {
    let cwd = std::env::current_dir()?;
    let tmp_path = cwd.join(filename);
    let config_file_path = tmp_path.to_str().context(format!(
        "Could not create String from Path {}",
        tmp_path.display()
    ))?;

    log::info!("Loading config file {}", config_file_path);

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

    for server in &config.servers {
        validate_server_timeout(&server.name, server.timeout)?;
        validate_readiness_url(&server.name, &server.url)?;
    }

    Ok(config)
}
