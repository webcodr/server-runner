use anyhow::{bail, Context};
use log::info;
use std::env;

use crate::constants::DEFAULT_TIMEOUT_SECONDS;

#[derive(serde::Deserialize, Clone)]
pub struct Server {
    pub name: String,
    pub url: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

#[derive(serde::Deserialize, Clone)]
pub struct Config {
    pub servers: Vec<Server>,
    pub command: String,
}

pub fn get_config(filename: &str) -> anyhow::Result<Config> {
    get_config_with_logging(filename, true)
}

pub fn get_config_with_logging(filename: &str, enable_logging: bool) -> anyhow::Result<Config> {
    let cwd = env::current_dir()?;
    let tmp_path = cwd.join(filename);
    let config_file_path = tmp_path.to_str().context(format!(
        "Could not create String from Path {}",
        tmp_path.display()
    ))?;

    if enable_logging {
        info!("Loading config file {}", config_file_path);
    }

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

    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &Config) -> anyhow::Result<()> {
    for server in &config.servers {
        if server.name.trim().is_empty() {
            bail!("Server name cannot be empty");
        }
        
        if server.url.trim().is_empty() {
            bail!("Server URL cannot be empty");
        }
        
        if server.command.trim().is_empty() {
            bail!("Server command cannot be empty");
        }
        
        if server.timeout == 0 {
            bail!("Server timeout must be greater than 0");
        }
        
        if server.timeout > 300 {
            bail!("Server timeout cannot exceed 300 seconds");
        }
    }
    
    Ok(())
}