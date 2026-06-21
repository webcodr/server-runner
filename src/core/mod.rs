pub mod actor;
pub mod command;
pub mod health;
pub mod output;
pub mod server;
pub mod state;

use log::info;

use std::collections::HashMap;

use crate::config::Server;
use crate::core::server::ServerProcess;
use crate::core::state::{Attempts, ServerName, ServerStatus};

/// Owns the running server processes and tracks attempts.
pub struct Engine {
    pub processes: Vec<ServerProcess>,
    attempts: HashMap<ServerName, Attempts>,
    max_attempts: u8,
}

impl Engine {
    /// Spawn all servers and start capturing their output.
    pub fn start(servers: &[Server], max_attempts: u8) -> anyhow::Result<Self> {
        let mut processes = Vec::with_capacity(servers.len());
        for s in servers {
            info!("Starting server {}", s.name);
            processes.push(ServerProcess::spawn(&s.name, &s.command)?);
        }
        Ok(Self {
            processes,
            attempts: HashMap::new(),
            max_attempts,
        })
    }

    /// Probe one server, incrementing its attempt count.
    /// Returns Err when attempts are exhausted (plain-mode abort contract).
    pub async fn probe(&mut self, server: &Server) -> anyhow::Result<ServerStatus> {
        let attempts = self
            .attempts
            .entry(ServerName(server.name.clone()))
            .and_modify(|a| *a += 1)
            .or_insert(Attempts(1));

        if attempts.0 >= self.max_attempts {
            let word = if self.max_attempts == 1 {
                "attempt"
            } else {
                "attempts"
            };
            anyhow::bail!(
                "Could not connect to server {} after {} {}",
                server.name,
                attempts,
                word
            );
        }

        info!(
            "Checking server {} on url {}, attempt {}, waiting one second ...",
            server.name, server.url, attempts
        );

        health::check(&server.name, &server.url, server.timeout).await
    }

    /// Stop all server process groups.
    pub async fn stop_all(&mut self) -> anyhow::Result<()> {
        for p in self.processes.iter_mut() {
            info!("Stopping server {}", p.name);
            p.stop().await?;
        }
        info!("All servers stopped successfully");
        Ok(())
    }
}
