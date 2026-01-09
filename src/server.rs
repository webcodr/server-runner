use crate::config::{Config, Server};
use crate::process::{ServerProcess, run_command};
use anyhow::{Context, bail};
use log::info;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum ServerStatus {
    Waiting,
    Ready,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServerName(pub String);

pub struct ServerManager {
    pub config: Config,
    processes: Arc<Mutex<Vec<ServerProcess>>>,
    statuses: Arc<Mutex<HashMap<ServerName, ServerStatus>>>,
    attempts: HashMap<ServerName, u32>,
}

impl ServerManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            processes: Arc::new(Mutex::new(Vec::new())),
            statuses: Arc::new(Mutex::new(HashMap::new())),
            attempts: HashMap::new(),
        }
    }

    pub fn get_processes_arc(&self) -> Arc<Mutex<Vec<ServerProcess>>> {
        Arc::clone(&self.processes)
    }

    pub fn start_servers(
        &mut self,
        max_attempts: u32,
        poll_interval: u64,
        fail_fast: bool,
    ) -> anyhow::Result<()> {
        // Clone servers to avoid borrow issues
        let servers = self.config.servers.clone();

        // Group servers by priority
        let mut priority_groups: HashMap<u32, Vec<Server>> = HashMap::new();
        for server in servers {
            priority_groups
                .entry(server.priority)
                .or_default()
                .push(server);
        }

        let mut sorted_priorities: Vec<u32> = priority_groups.keys().copied().collect();
        sorted_priorities.sort();

        // Start servers by priority groups
        for priority in sorted_priorities {
            if let Some(servers) = priority_groups.get(&priority) {
                info!(
                    "Starting priority {} servers: {}",
                    priority,
                    servers
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );

                self.start_server_group(servers, max_attempts, poll_interval, fail_fast)?;
            }
        }

        Ok(())
    }

    fn start_server_group(
        &mut self,
        servers: &[Server],
        max_attempts: u32,
        poll_interval: u64,
        fail_fast: bool,
    ) -> anyhow::Result<()> {
        // Check dependencies are ready
        for server in servers {
            for dep_name in &server.depends_on {
                let status = {
                    let statuses = self.statuses.lock().unwrap();
                    statuses.get(&ServerName(dep_name.clone())).cloned()
                };

                if status != Some(ServerStatus::Ready) {
                    bail!(
                        "Server '{}' depends on '{}' which is not ready",
                        server.name,
                        dep_name
                    );
                }
            }
        }

        // Start all servers in this group
        for server in servers {
            self.start_single_server(server)?;
        }

        // Wait for all servers in this group to be ready
        self.wait_for_servers(servers, max_attempts, poll_interval, fail_fast)?;

        Ok(())
    }

    fn start_single_server(&mut self, server: &Server) -> anyhow::Result<()> {
        // Run before_start hook
        if let Some(hook) = &server.hooks.before_start {
            info!("Running before_start hook for server {}", server.name);
            let mut process = run_command(hook, &server.env, &server.output).context(format!(
                "Failed to run before_start hook for {}",
                server.name
            ))?;
            process.wait()?;
        }

        // Apply startup delay
        if server.startup_delay > 0 {
            info!(
                "Waiting {} seconds before starting server {}",
                server.startup_delay, server.name
            );
            thread::sleep(Duration::from_secs(server.startup_delay));
        }

        info!("Starting server {}", server.name);

        let process = run_command(&server.command, &server.env, &server.output)?;
        let server_process = ServerProcess {
            name: server.name.clone(),
            process,
        };

        self.processes.lock().unwrap().push(server_process);
        self.statuses
            .lock()
            .unwrap()
            .insert(ServerName(server.name.clone()), ServerStatus::Waiting);

        Ok(())
    }

    fn wait_for_servers(
        &mut self,
        servers: &[Server],
        max_attempts: u32,
        poll_interval: u64,
        fail_fast: bool,
    ) -> anyhow::Result<()> {
        let mut errors: Vec<anyhow::Error> = Vec::new();

        loop {
            let mut all_ready = true;

            for server in servers {
                let server_name = ServerName(server.name.clone());

                let current_status = {
                    let statuses = self.statuses.lock().unwrap();
                    statuses.get(&server_name).cloned()
                };

                // Skip servers that are already ready or have already failed
                if current_status == Some(ServerStatus::Ready) {
                    continue;
                }

                if current_status == Some(ServerStatus::Failed) {
                    all_ready = false;
                    continue;
                }

                match self.check_server(server, max_attempts) {
                    Ok(status) => {
                        self.statuses
                            .lock()
                            .unwrap()
                            .insert(server_name.clone(), status.clone());

                        if status == ServerStatus::Ready {
                            info!("Server {} is ready", server.name);

                            // Run after_ready hook
                            if let Some(hook) = &server.hooks.after_ready {
                                info!("Running after_ready hook for server {}", server.name);
                                let mut process = run_command(hook, &server.env, &server.output)
                                    .context(format!(
                                        "Failed to run after_ready hook for {}",
                                        server.name
                                    ))?;
                                process.wait()?;
                            }
                        } else if status == ServerStatus::Waiting {
                            all_ready = false;
                        }
                    }
                    Err(e) => {
                        self.statuses
                            .lock()
                            .unwrap()
                            .insert(server_name, ServerStatus::Failed);

                        all_ready = false;

                        if fail_fast {
                            return Err(e);
                        }

                        errors.push(e);
                    }
                }
            }

            if all_ready {
                return Ok(());
            }

            // If we collected any errors, return them now
            if !errors.is_empty() {
                // Check if all servers are now in a final state
                let all_final = servers.iter().all(|s| {
                    let status = self
                        .statuses
                        .lock()
                        .unwrap()
                        .get(&ServerName(s.name.clone()))
                        .cloned();
                    status == Some(ServerStatus::Ready) || status == Some(ServerStatus::Failed)
                });

                if all_final {
                    if errors.len() == 1 {
                        return Err(errors.into_iter().next().unwrap());
                    } else {
                        bail!("{} servers failed to start", errors.len());
                    }
                }
            }

            thread::sleep(Duration::from_secs(poll_interval));
        }
    }

    fn check_server(&mut self, server: &Server, max_attempts: u32) -> anyhow::Result<ServerStatus> {
        let server_name = ServerName(server.name.clone());
        let attempts = self
            .attempts
            .entry(server_name)
            .and_modify(|a| *a += 1)
            .or_insert(1);

        if *attempts > max_attempts {
            let attempt_word = if max_attempts == 1 {
                "attempt"
            } else {
                "attempts"
            };
            bail!(
                "Could not connect to server {} after {} {}",
                server.name,
                max_attempts,
                attempt_word
            );
        }

        info!(
            "Checking server {} on url {}, attempt {}, waiting {} second(s) ...",
            server.name, server.url, attempts, server.retry_interval
        );

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(server.timeout))
            .build()?;

        let mut request = match server.health_check.method.as_str() {
            "GET" => client.get(&server.url),
            "POST" => client.post(&server.url),
            "HEAD" => client.head(&server.url),
            "PUT" => client.put(&server.url),
            "PATCH" => client.patch(&server.url),
            "DELETE" => client.delete(&server.url),
            _ => client.get(&server.url),
        };

        // Add custom headers
        for (key, value) in &server.health_check.headers {
            request = request.header(key, value);
        }

        match request.send() {
            Ok(response) => {
                let status = response.status().as_u16();

                if server.health_check.expected_status.contains(&status) {
                    Ok(ServerStatus::Ready)
                } else if status >= 500 {
                    // Server error - this is a fatal error
                    bail!(
                        "Server {} returned error status {}, this indicates a server problem",
                        server.name,
                        status
                    );
                } else {
                    // Client error or unexpected status - keep waiting
                    Ok(ServerStatus::Waiting)
                }
            }
            Err(error) => {
                if error.is_connect() || error.is_timeout() {
                    // Connection refused or timeout - server not ready yet
                    Ok(ServerStatus::Waiting)
                } else {
                    // Other error - could be DNS, TLS, etc.
                    bail!(
                        "Could not connect to server {} on url {}: {}",
                        server.name,
                        server.url,
                        error
                    );
                }
            }
        }
    }

    pub fn run_final_command(&self) -> anyhow::Result<()> {
        let command = &self.config.command;

        info!("Running final command: {}", command);

        let mut process = run_command(command, &self.config.env, &Default::default())
            .context(format!("Could not start process {}", command))?;

        let status = process.wait()?;

        if status.success() {
            info!("Command {} finished successfully", command);
            Ok(())
        } else {
            bail!(
                "Command {} failed with status: {}",
                command,
                status.code().unwrap_or(-1)
            );
        }
    }

    pub fn stop_all_servers(&self) -> anyhow::Result<()> {
        let mut processes = self.processes.lock().unwrap();
        stop_servers(&mut processes, &self.config.servers)
    }
}

pub fn stop_servers(
    processes: &mut Vec<ServerProcess>,
    server_configs: &[Server],
) -> anyhow::Result<()> {
    // Build a map of server names to their configs
    let config_map: HashMap<&str, &Server> = server_configs
        .iter()
        .map(|s| (s.name.as_str(), s))
        .collect();

    for process in processes.iter_mut() {
        // Run before_stop hook if configured
        if let Some(server_config) = config_map.get(process.name.as_str()) {
            if let Some(hook) = &server_config.hooks.before_stop {
                info!("Running before_stop hook for server {}", process.name);
                if let Ok(mut hook_process) =
                    run_command(hook, &server_config.env, &server_config.output)
                {
                    let _ = hook_process.wait();
                }
            }
        }

        info!("Stopping server {}", process.name);

        if process.process.kill().is_ok() {
            let _ = process.process.wait();
        } else {
            bail!("Failed to stop process {}", process.name);
        }
    }

    info!("All servers stopped successfully");

    Ok(())
}
