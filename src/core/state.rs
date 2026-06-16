use std::collections::VecDeque;
use std::fmt;
use std::ops::AddAssign;
use std::sync::{Arc, Mutex};

use crate::config::Server;
use crate::core::server::LOG_CAPACITY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStatus {
    Waiting,
    Running,
    #[allow(dead_code)] // used by TUI in Plan 2
    Failed,
    #[allow(dead_code)] // used by TUI in Plan 2
    Stopped,
}

#[derive(Copy, Clone, Debug)]
pub struct Attempts(pub u8);

impl AddAssign<u8> for Attempts {
    fn add_assign(&mut self, other: u8) {
        self.0 = self.0.saturating_add(other);
    }
}

impl fmt::Display for Attempts {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<u8> for Attempts {
    fn eq(&self, other: &u8) -> bool {
        self.0 == *other
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServerName(pub String);

/// Bounded FIFO line buffer. Oldest lines drop once `capacity` is exceeded.
pub struct RingBuffer {
    lines: VecDeque<String>,
    capacity: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(capacity.min(1024)),
            capacity,
        }
    }

    pub fn push(&mut self, line: String) {
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    #[allow(dead_code)] // used by AppState in Task 9
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[allow(dead_code)] // used by AppState in Task 9
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    #[allow(dead_code)] // used by AppState in Task 9
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.lines.iter()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalCmdStatus {
    Idle,
    #[allow(dead_code)] // used by TUI in Plan 2
    Running,
    #[allow(dead_code)] // used by TUI in Plan 2
    Succeeded(i32),
    #[allow(dead_code)] // used by TUI in Plan 2
    Failed(i32),
}

pub struct ServerView {
    #[allow(dead_code)] // used by TUI in Plan 2
    pub name: String,
    #[allow(dead_code)] // used by TUI in Plan 2
    pub url: String,
    #[allow(dead_code)] // used by TUI in Plan 2
    pub status: ServerStatus,
    #[allow(dead_code)] // used by TUI in Plan 2
    pub attempts: Attempts,
    #[allow(dead_code)] // used by TUI in Plan 2
    pub log: Arc<Mutex<RingBuffer>>,
}

pub struct FinalCmdView {
    #[allow(dead_code)] // used by TUI in Plan 2
    pub command: String,
    #[allow(dead_code)] // used by TUI in Plan 2
    pub status: FinalCmdStatus,
    #[allow(dead_code)] // used by TUI in Plan 2
    pub log: Arc<Mutex<RingBuffer>>,
}

/// Shared, render-friendly snapshot of the engine, owned behind Arc<Mutex<_>>.
pub struct AppState {
    pub servers: Vec<ServerView>,
    #[allow(dead_code)] // used by TUI in Plan 2
    pub final_cmd: FinalCmdView,
}

impl AppState {
    pub fn new(servers: &[Server], command: &str) -> Self {
        Self {
            servers: servers
                .iter()
                .map(|server| ServerView {
                    name: server.name.clone(),
                    url: server.url.clone(),
                    status: ServerStatus::Waiting,
                    attempts: Attempts(0),
                    log: Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY))),
                })
                .collect(),
            final_cmd: FinalCmdView {
                command: command.to_string(),
                status: FinalCmdStatus::Idle,
                log: Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY))),
            },
        }
    }

    pub fn selectable_len(&self) -> usize {
        self.servers.len() + 1
    }

    pub fn is_final_selection(&self, selected: usize) -> bool {
        selected == self.servers.len()
    }
}

const _: fn(&[Server], &str) -> AppState = AppState::new;
const _: fn(&AppState) -> usize = AppState::selectable_len;
const _: fn(&AppState, usize) -> bool = AppState::is_final_selection;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_saturate_at_u8_max() {
        let mut a = Attempts(254);
        a += 1;
        a += 1;
        a += 1;
        assert_eq!(a, 255u8);
    }

    #[test]
    fn ring_buffer_drops_oldest_when_full() {
        let mut rb = RingBuffer::new(2);
        rb.push("a".into());
        rb.push("b".into());
        rb.push("c".into());
        assert_eq!(rb.len(), 2);
        let got: Vec<_> = rb.iter().cloned().collect();
        assert_eq!(got, vec!["b".to_string(), "c".to_string()]);
    }
}

#[cfg(test)]
mod app_state_tests {
    use super::*;

    #[test]
    fn final_cmd_status_equality() {
        assert_eq!(FinalCmdStatus::Succeeded(0), FinalCmdStatus::Succeeded(0));
        assert_ne!(FinalCmdStatus::Succeeded(0), FinalCmdStatus::Failed(1));
    }

    #[test]
    fn app_state_initializes_servers_and_final_command() {
        let servers = vec![
            crate::config::Server {
                name: "API".to_string(),
                url: "http://127.0.0.1:3000".to_string(),
                command: "python3 -m http.server 3000".to_string(),
                timeout: 5,
            },
            crate::config::Server {
                name: "Worker".to_string(),
                url: "http://127.0.0.1:3001".to_string(),
                command: "python3 -m http.server 3001".to_string(),
                timeout: 3,
            },
        ];

        let app = AppState::new(&servers, "npm test");

        assert_eq!(app.servers.len(), 2);
        assert_eq!(app.servers[0].name, "API");
        assert_eq!(app.servers[0].url, "http://127.0.0.1:3000");
        assert_eq!(app.servers[0].status, ServerStatus::Waiting);
        assert_eq!(app.servers[0].attempts, 0u8);
        assert_eq!(app.final_cmd.command, "npm test");
        assert_eq!(app.final_cmd.status, FinalCmdStatus::Idle);
    }

    #[test]
    fn app_state_counts_selectable_rows() {
        let servers = vec![crate::config::Server {
            name: "API".to_string(),
            url: "http://127.0.0.1:3000".to_string(),
            command: "python3 -m http.server 3000".to_string(),
            timeout: 5,
        }];

        let app = AppState::new(&servers, "npm test");

        assert_eq!(app.selectable_len(), 2);
        assert!(app.is_final_selection(1));
        assert!(!app.is_final_selection(0));
    }
}
