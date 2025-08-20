use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader},
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use crate::{
    attempts::Attempts,
    config::Config,
    constants::{DEFAULT_MAX_ATTEMPTS, MAX_OUTPUT_LINES_PER_SERVER},
    server_management::{ServerProcess, ServerStatus},
};

pub struct TuiApp {
    config: Config,
    server_statuses: Arc<Mutex<HashMap<String, ServerStatus>>>,
    server_processes: Arc<Mutex<Vec<ServerProcess>>>,
    server_outputs: Arc<Mutex<HashMap<String, Vec<String>>>>,
    command_output: Arc<Mutex<Vec<String>>>,
    should_quit: bool,
    servers_started: bool,
    tx: Option<Sender<TuiMessage>>,
    rx: Option<Receiver<TuiMessage>>,
}

#[derive(Debug)]
pub enum TuiMessage {
    ServerStatusUpdate(String, ServerStatus),
    ServerOutput(String, String), // server_name, output_line
    CommandOutput(String),
    ServersReady,
    Error(String),
}

impl TuiApp {
    pub fn new(config: Config) -> Self {
        let (tx, rx) = mpsc::channel();
        
        Self {
            config,
            server_statuses: Arc::new(Mutex::new(HashMap::new())),
            server_processes: Arc::new(Mutex::new(Vec::new())),
            server_outputs: Arc::new(Mutex::new(HashMap::new())),
            command_output: Arc::new(Mutex::new(Vec::new())),
            should_quit: false,
            servers_started: false,
            tx: Some(tx),
            rx: Some(rx),
        }
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_app(&mut terminal);

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    fn run_app(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') => {
                                self.should_quit = true;
                                self.shutdown_servers()?;
                                break;
                            }
                            KeyCode::Char('s') => {
                                if !self.servers_started {
                                    self.start_servers()?;
                                }
                            }
                            KeyCode::Char('r') => {
                                if self.servers_started {
                                    self.restart_servers()?;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            if let Some(rx) = self.rx.as_ref() {
                let messages: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
                for message in messages {
                    self.handle_message(message)?;
                }
            }

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    fn ui(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(25), Constraint::Percentage(65), Constraint::Length(3)].as_ref())
            .split(f.area());

        let middle_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
            .split(chunks[1]);

        self.render_servers_panel(f, chunks[0]);
        self.render_server_outputs_panel(f, middle_chunks[0]);
        self.render_command_panel(f, middle_chunks[1]);
        self.render_hotkey_legend(f, chunks[2]);
    }

    fn render_servers_panel(&self, f: &mut Frame, area: Rect) {
        let servers_block = Block::default()
            .title("Servers")
            .borders(Borders::ALL);

        let mut items = Vec::new();
        

        for server in &self.config.servers {
            let status = self.server_statuses
                .lock()
                .unwrap()
                .get(&server.name)
                .cloned()
                .unwrap_or(ServerStatus::Waiting);

            let (status_text, color) = match status {
                ServerStatus::Waiting => ("Waiting", Color::Yellow),
                ServerStatus::Running => ("Running", Color::Green),
            };

            items.push(ListItem::new(Line::from(vec![
                Span::styled(&server.name, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" - "),
                Span::styled(status_text, Style::default().fg(color)),
                Span::raw(" | "),
                Span::styled(&server.url, Style::default().fg(Color::Cyan)),
            ])));
        }

        let servers_list = List::new(items).block(servers_block);
        f.render_widget(servers_list, area);
    }

    fn render_server_outputs_panel(&self, f: &mut Frame, area: Rect) {
        let outputs_block = Block::default()
            .title("Server Outputs")
            .borders(Borders::ALL);

        let mut text = Vec::new();
        
        {
            let server_outputs = self.server_outputs.lock().unwrap();
            if server_outputs.is_empty() {
                text.push(Line::from(vec![
                    Span::styled("No server output yet", Style::default().fg(Color::Gray))
                ]));
            } else {
                for (server_name, lines) in server_outputs.iter() {
                    text.push(Line::from(vec![
                        Span::styled(format!("[{}]", server_name), Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD))
                    ]));
                    
                    // Show last few lines for each server
                    let start_idx = if lines.len() > MAX_OUTPUT_LINES_PER_SERVER { 
                        lines.len() - MAX_OUTPUT_LINES_PER_SERVER 
                    } else { 
                        0 
                    };
                    for line in &lines[start_idx..] {
                        text.push(Line::from(vec![
                            Span::raw("  "),
                            Span::raw(line.clone())
                        ]));
                    }
                    text.push(Line::from(""));
                }
            }
        }

        let outputs_paragraph = Paragraph::new(Text::from(text)).block(outputs_block);
        f.render_widget(outputs_paragraph, area);
    }

    fn render_command_panel(&self, f: &mut Frame, area: Rect) {
        let command_block = Block::default()
            .title("Final Command")
            .borders(Borders::ALL);

        let mut text = Vec::new();
        
        text.push(Line::from(vec![
            Span::styled("Command: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(&self.config.command, Style::default().fg(Color::Cyan)),
        ]));
        
        text.push(Line::from(""));
        
        if self.servers_started {
            text.push(Line::from(vec![
                Span::styled("Output:", Style::default().add_modifier(Modifier::BOLD))
            ]));
            
            {
                let output = self.command_output.lock().unwrap();
                for line in output.iter() {
                    text.push(Line::from(line.clone()));
                }
            }
        } else {
            text.push(Line::from(vec![
                Span::styled("Command will run when all servers are ready", Style::default().fg(Color::Yellow))
            ]));
        }

        let command_paragraph = Paragraph::new(Text::from(text)).block(command_block);
        f.render_widget(command_paragraph, area);
    }

    fn start_servers(&mut self) -> Result<()> {
        self.servers_started = true;
        
        let tx = self.tx.as_ref().unwrap().clone();
        let config = self.config.clone();
        let server_statuses = Arc::clone(&self.server_statuses);
        let server_processes = Arc::clone(&self.server_processes);
        let server_outputs = Arc::clone(&self.server_outputs);
        let command_output = Arc::clone(&self.command_output);

        thread::spawn(move || {
            if let Err(e) = Self::run_servers_background(
                config,
                server_statuses,
                server_processes,
                server_outputs,
                command_output,
                tx,
            ) {
                log::error!("Error running servers: {}", e);
            }
        });

        Ok(())
    }

    fn run_servers_background(
        config: Config,
        server_statuses: Arc<Mutex<HashMap<String, ServerStatus>>>,
        server_processes: Arc<Mutex<Vec<ServerProcess>>>,
        server_outputs: Arc<Mutex<HashMap<String, Vec<String>>>>,
        command_output: Arc<Mutex<Vec<String>>>,
        tx: Sender<TuiMessage>,
    ) -> Result<()> {
        use crate::server_management::{start_servers, wait_for_servers, execute_command, cleanup_processes};

        let mut processes = start_servers(&config.servers, false)?;
        
        // Start output capture threads for each server
        for process in &mut processes {
            let server_name = process.name.clone();
            let tx_clone = tx.clone();
            
            // Initialize server output storage
            server_outputs.lock().unwrap().insert(server_name.clone(), Vec::new());
            
            // Capture stdout if available
            if let Some(stdout) = process.stdout_reader.take() {
                let stdout_reader = BufReader::new(stdout);
                let server_name_clone = server_name.clone();
                let tx_stdout = tx_clone.clone();
                
                thread::spawn(move || {
                    for line in stdout_reader.lines() {
                        if let Ok(line) = line {
                            let _ = tx_stdout.send(TuiMessage::ServerOutput(server_name_clone.clone(), line));
                        }
                    }
                });
            }
            
            // Capture stderr if available
            if let Some(stderr) = process.stderr_reader.take() {
                let stderr_reader = BufReader::new(stderr);
                let server_name_clone = server_name.clone();
                let tx_stderr = tx_clone.clone();
                
                thread::spawn(move || {
                    for line in stderr_reader.lines() {
                        if let Ok(line) = line {
                            let _ = tx_stderr.send(TuiMessage::ServerOutput(server_name_clone.clone(), format!("[STDERR] {}", line)));
                        }
                    }
                });
            }
        }
        
        {
            let mut server_processes_guard = server_processes.lock().unwrap();
            *server_processes_guard = processes;
        }

        for server in &config.servers {
            server_statuses.lock().unwrap().insert(server.name.clone(), ServerStatus::Waiting);
            tx.send(TuiMessage::ServerStatusUpdate(server.name.clone(), ServerStatus::Waiting))?;
        }

        let max_attempts = Attempts::new(DEFAULT_MAX_ATTEMPTS);
        match wait_for_servers(&config.servers, max_attempts, false) {
            Ok(_) => {
                for server in &config.servers {
                    server_statuses.lock().unwrap().insert(server.name.clone(), ServerStatus::Running);
                    tx.send(TuiMessage::ServerStatusUpdate(server.name.clone(), ServerStatus::Running))?;
                }
                
                tx.send(TuiMessage::ServersReady)?;
                
                match execute_command(&config.command) {
                    Ok(output) => {
                        let mut command_output_guard = command_output.lock().unwrap();
                        command_output_guard.push(format!("Exit code: {}", output.status.code().unwrap_or(-1)));
                        
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        
                        for line in stdout.lines() {
                            command_output_guard.push(line.to_string());
                            tx.send(TuiMessage::CommandOutput(line.to_string()))?;
                        }
                        
                        if !stderr.is_empty() {
                            command_output_guard.push("STDERR:".to_string());
                            for line in stderr.lines() {
                                command_output_guard.push(line.to_string());
                                tx.send(TuiMessage::CommandOutput(line.to_string()))?;
                            }
                        }
                    }
                    Err(e) => {
                        let error_msg = format!("Error executing command: {}", e);
                        command_output.lock().unwrap().push(error_msg.clone());
                        tx.send(TuiMessage::Error(error_msg))?;
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to start servers: {}", e);
                tx.send(TuiMessage::Error(error_msg))?;
            }
        }

        cleanup_processes(&mut server_processes.lock().unwrap(), false)?;
        Ok(())
    }

    fn handle_message(&mut self, message: TuiMessage) -> Result<()> {
        match message {
            TuiMessage::ServerStatusUpdate(name, status) => {
                self.server_statuses.lock().unwrap().insert(name, status);
            }
            TuiMessage::ServerOutput(server_name, line) => {
                self.server_outputs.lock().unwrap()
                    .entry(server_name)
                    .or_insert_with(Vec::new)
                    .push(line);
            }
            TuiMessage::CommandOutput(_line) => {
                // Output is already handled in the background thread
            }
            TuiMessage::ServersReady => {
                // All servers are ready, command will execute
            }
            TuiMessage::Error(error) => {
                self.command_output.lock().unwrap().push(format!("ERROR: {}", error));
            }
        }
        Ok(())
    }

    fn shutdown_servers(&self) -> Result<()> {
        use crate::server_management::cleanup_processes;
        
        let mut processes = self.server_processes.lock().unwrap();
        cleanup_processes(&mut processes, false)?;
        Ok(())
    }

    fn restart_servers(&mut self) -> Result<()> {
        // Stop all current servers
        self.shutdown_servers()?;
        
        // Clear all state
        self.server_statuses.lock().unwrap().clear();
        self.server_outputs.lock().unwrap().clear();
        self.command_output.lock().unwrap().clear();
        self.servers_started = false;
        
        // Start servers again
        self.start_servers()?;
        
        Ok(())
    }

    fn render_hotkey_legend(&self, f: &mut Frame, area: Rect) {
        let legend_block = Block::default()
            .title("Hot Keys")
            .borders(Borders::ALL);

        let mut legend_text = Vec::new();
        
        if !self.servers_started {
            legend_text.push(Line::from(vec![
                Span::styled("[s]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" Start servers  "),
                Span::styled("[q]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" Quit"),
            ]));
        } else {
            legend_text.push(Line::from(vec![
                Span::styled("[r]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" Restart servers  "),
                Span::styled("[q]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" Quit"),
            ]));
        }

        let legend_paragraph = Paragraph::new(Text::from(legend_text)).block(legend_block);
        f.render_widget(legend_paragraph, area);
    }
}