use anyhow::{Context, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::{env, fs};

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub servers: Vec<Server>,
    pub command: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Server {
    pub name: String,
    pub url: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub health_check: HealthCheck,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_retry_interval")]
    pub retry_interval: u64,
    #[serde(default)]
    pub startup_delay: u64,
    #[serde(default)]
    pub hooks: Hooks,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default = "default_priority")]
    pub priority: u32,
    #[serde(default)]
    pub output: OutputConfig,
}

#[derive(Deserialize, Clone, Debug)]
pub struct HealthCheck {
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default = "default_expected_status")]
    pub expected_status: Vec<u16>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self {
            method: default_method(),
            expected_status: default_expected_status(),
            headers: HashMap::new(),
        }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Hooks {
    #[serde(default)]
    pub before_start: Option<String>,
    #[serde(default)]
    pub after_ready: Option<String>,
    #[serde(default)]
    pub before_stop: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct OutputConfig {
    #[serde(default = "default_output_mode")]
    pub mode: String,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            mode: default_output_mode(),
            stdout: None,
            stderr: None,
            prefix: None,
        }
    }
}

fn default_timeout() -> u64 {
    5
}

fn default_retry_interval() -> u64 {
    1
}

fn default_priority() -> u32 {
    0
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_expected_status() -> Vec<u16> {
    vec![200]
}

fn default_output_mode() -> String {
    "inherit".to_string()
}

pub fn load_config(filename: &str) -> anyhow::Result<Config> {
    let cwd = env::current_dir()?;
    let tmp_path = cwd.join(filename);
    let config_file_path = tmp_path.to_str().context(format!(
        "Could not create String from Path {}",
        tmp_path.display()
    ))?;

    log::info!("Loading config file {}", config_file_path);

    let content = fs::read_to_string(config_file_path)
        .context(format!("Could not find config file {}", filename))?;

    // Expand environment variables
    let expanded_content = expand_env_vars(&content);

    let config: Config = serde_yaml::from_str(&expanded_content)
        .context(format!("Could not parse config file {}", filename))?;

    validate_config(&config)?;

    Ok(config)
}

fn expand_env_vars(content: &str) -> String {
    let mut result = content.to_string();

    // Simple environment variable expansion: ${VAR_NAME}
    let re = regex::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();

    loop {
        let mut changed = false;
        let new_result = re
            .replace_all(&result, |caps: &regex::Captures| {
                let var_name = &caps[1];
                if let Ok(value) = env::var(var_name) {
                    changed = true;
                    value
                } else {
                    caps[0].to_string() // Keep original if not found
                }
            })
            .to_string();

        if !changed {
            break;
        }
        result = new_result;
    }

    result
}

fn validate_config(config: &Config) -> anyhow::Result<()> {
    if config.servers.is_empty() {
        bail!("Configuration must include at least one server");
    }

    if config.command.trim().is_empty() {
        bail!("Configuration must include a command to run");
    }

    // Validate server dependencies
    let server_names: Vec<&str> = config.servers.iter().map(|s| s.name.as_str()).collect();

    for server in &config.servers {
        for dep in &server.depends_on {
            if !server_names.contains(&dep.as_str()) {
                bail!(
                    "Server '{}' depends on '{}' which does not exist",
                    server.name,
                    dep
                );
            }
            if dep == &server.name {
                bail!("Server '{}' cannot depend on itself", server.name);
            }
        }

        // Validate health check method
        let valid_methods = ["GET", "POST", "HEAD", "PUT", "PATCH", "DELETE"];
        if !valid_methods.contains(&server.health_check.method.as_str()) {
            bail!(
                "Invalid health check method '{}' for server '{}'. Must be one of: {}",
                server.health_check.method,
                server.name,
                valid_methods.join(", ")
            );
        }

        // Validate output mode
        let valid_modes = ["inherit", "capture", "null", "file"];
        if !valid_modes.contains(&server.output.mode.as_str()) {
            bail!(
                "Invalid output mode '{}' for server '{}'. Must be one of: {}",
                server.output.mode,
                server.name,
                valid_modes.join(", ")
            );
        }

        if server.output.mode == "file"
            && server.output.stdout.is_none()
            && server.output.stderr.is_none()
        {
            bail!(
                "Server '{}' has output mode 'file' but no stdout or stderr path specified",
                server.name
            );
        }
    }

    // Check for circular dependencies
    detect_circular_dependencies(config)?;

    Ok(())
}

fn detect_circular_dependencies(config: &Config) -> anyhow::Result<()> {
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();

    for server in &config.servers {
        graph.insert(
            &server.name,
            server.depends_on.iter().map(|s| s.as_str()).collect(),
        );
    }

    for server in &config.servers {
        let mut visited = Vec::new();
        if has_cycle(&graph, &server.name, &mut visited) {
            bail!(
                "Circular dependency detected involving server '{}': {}",
                server.name,
                visited.join(" -> ")
            );
        }
    }

    Ok(())
}

fn has_cycle<'a>(
    graph: &HashMap<&'a str, Vec<&'a str>>,
    node: &'a str,
    visited: &mut Vec<&'a str>,
) -> bool {
    if visited.contains(&node) {
        visited.push(node);
        return true;
    }

    visited.push(node);

    if let Some(deps) = graph.get(node) {
        for dep in deps {
            if has_cycle(graph, dep, visited) {
                return true;
            }
        }
    }

    visited.pop();
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_vars() {
        unsafe {
            env::set_var("TEST_VAR", "test_value");
        }
        let input = "url: http://localhost:${TEST_VAR}/health";
        let output = expand_env_vars(input);
        assert_eq!(output, "url: http://localhost:test_value/health");
    }

    #[test]
    fn test_circular_dependency_detection() {
        let config = Config {
            servers: vec![
                Server {
                    name: "A".to_string(),
                    url: "http://localhost:8001".to_string(),
                    command: "echo A".to_string(),
                    timeout: 5,
                    health_check: HealthCheck::default(),
                    env: HashMap::new(),
                    retry_interval: 1,
                    startup_delay: 0,
                    hooks: Hooks::default(),
                    depends_on: vec!["B".to_string()],
                    priority: 0,
                    output: OutputConfig::default(),
                },
                Server {
                    name: "B".to_string(),
                    url: "http://localhost:8002".to_string(),
                    command: "echo B".to_string(),
                    timeout: 5,
                    health_check: HealthCheck::default(),
                    env: HashMap::new(),
                    retry_interval: 1,
                    startup_delay: 0,
                    hooks: Hooks::default(),
                    depends_on: vec!["A".to_string()],
                    priority: 0,
                    output: OutputConfig::default(),
                },
            ],
            command: "echo done".to_string(),
            env: HashMap::new(),
        };

        assert!(detect_circular_dependencies(&config).is_err());
    }
}
