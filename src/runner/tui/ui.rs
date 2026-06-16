#![allow(dead_code)] // wired into the TUI runtime in a later Plan 2 task

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::{Block, Paragraph};
use std::sync::{Arc, Mutex};

use crate::core::state::{
    AppState, FinalCmdStatus, FinalCmdView, RingBuffer, ServerStatus, ServerView,
};
use crate::runner::tui::app::TuiApp;

pub fn render(frame: &mut Frame, tui: &TuiApp, state: &Arc<Mutex<AppState>>) {
    let Ok(state) = state.lock() else {
        return;
    };

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(10)])
        .split(root[0]);

    let sidebar = sidebar_items(tui, &state).join("\n");
    frame.render_widget(
        Paragraph::new(sidebar).block(Block::bordered().title("server-runner")),
        body[0],
    );

    let detail = selected_log(tui, &state).join("\n");
    frame.render_widget(
        Paragraph::new(detail).block(Block::bordered().title("log")),
        body[1],
    );

    let footer = tui
        .footer_message()
        .unwrap_or("q quit | j/k select | PgUp/PgDn scroll | End tail");
    frame.render_widget(Paragraph::new(footer), root[1]);
}

fn sidebar_items(tui: &TuiApp, state: &AppState) -> Vec<String> {
    let mut items: Vec<String> = state
        .servers
        .iter()
        .enumerate()
        .map(|(index, server)| server_item(index, tui.selected(), server))
        .collect();
    items.push(final_cmd_item(
        state.servers.len(),
        tui.selected(),
        &state.final_cmd,
    ));
    items
}

fn server_item(index: usize, selected: usize, server: &ServerView) -> String {
    format!(
        "{} {:<4} {:<12} {}",
        selection_marker(index, selected),
        server_status(server.status),
        server.name,
        server.attempts
    )
}

fn final_cmd_item(index: usize, selected: usize, final_cmd: &FinalCmdView) -> String {
    format!(
        "{} {:<5} {}",
        selection_marker(index, selected),
        final_cmd_status(final_cmd.status),
        final_cmd.command
    )
}

fn selection_marker(index: usize, selected: usize) -> &'static str {
    if index == selected { ">" } else { " " }
}

fn server_status(status: ServerStatus) -> &'static str {
    match status {
        ServerStatus::Waiting => "WAIT",
        ServerStatus::Running => "RUN",
        ServerStatus::Failed => "FAIL",
        ServerStatus::Stopped => "STOP",
    }
}

fn final_cmd_status(status: FinalCmdStatus) -> String {
    match status {
        FinalCmdStatus::Idle => "IDLE".to_string(),
        FinalCmdStatus::Running => "RUN".to_string(),
        FinalCmdStatus::Succeeded(code) => format!("OK {code}"),
        FinalCmdStatus::Failed(code) => format!("ERR {code}"),
    }
}

fn selected_log(tui: &TuiApp, state: &AppState) -> Vec<String> {
    if state.is_final_selection(tui.selected()) {
        return log_lines(&state.final_cmd.log);
    }

    state
        .servers
        .get(tui.selected())
        .map(|server| log_lines(&server.log))
        .unwrap_or_default()
}

fn log_lines(log: &Arc<Mutex<RingBuffer>>) -> Vec<String> {
    let Ok(log) = log.lock() else {
        return Vec::new();
    };

    log.iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use std::sync::{Arc, Mutex};

    use crate::core::state::{
        AppState, Attempts, FinalCmdStatus, FinalCmdView, RingBuffer, ServerStatus, ServerView,
    };
    use crate::runner::tui::app::TuiApp;

    fn sample_state() -> Arc<Mutex<AppState>> {
        let api_log = Arc::new(Mutex::new(RingBuffer::new(10)));
        api_log.lock().unwrap().push("api ready".to_string());
        let worker_log = Arc::new(Mutex::new(RingBuffer::new(10)));
        worker_log.lock().unwrap().push("worker failed".to_string());
        let final_log = Arc::new(Mutex::new(RingBuffer::new(10)));
        final_log.lock().unwrap().push("tests passed".to_string());

        Arc::new(Mutex::new(AppState {
            servers: vec![
                ServerView {
                    name: "API".to_string(),
                    url: "http://localhost:3000".to_string(),
                    status: ServerStatus::Running,
                    attempts: Attempts(2),
                    log: api_log,
                },
                ServerView {
                    name: "Worker".to_string(),
                    url: "http://localhost:3001".to_string(),
                    status: ServerStatus::Failed,
                    attempts: Attempts(5),
                    log: worker_log,
                },
            ],
            final_cmd: FinalCmdView {
                command: "npm test".to_string(),
                status: FinalCmdStatus::Succeeded(0),
                log: final_log,
            },
        }))
    }

    fn render_to_string(tui: &TuiApp, state: &Arc<Mutex<AppState>>) -> String {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| super::render(frame, tui, state))
            .unwrap();

        buffer_to_string(terminal.backend().buffer())
    }

    #[allow(deprecated)]
    fn buffer_to_string(buffer: &Buffer) -> String {
        let mut output = String::new();
        for y in buffer.area.y..buffer.area.y + buffer.area.height {
            for x in buffer.area.x..buffer.area.x + buffer.area.width {
                output.push_str(buffer.get(x, y).symbol());
            }
            output.push('\n');
        }
        output
    }

    #[test]
    fn renders_sidebar_statuses_and_selected_log() {
        let tui = TuiApp::new(2);
        let state = sample_state();

        let output = render_to_string(&tui, &state);

        assert!(output.contains("server-runner"));
        assert!(output.contains("API"));
        assert!(output.contains("RUN"));
        assert!(output.contains("Worker"));
        assert!(output.contains("FAIL"));
        assert!(output.contains("npm test"));
        assert!(output.contains("OK 0"));
        assert!(output.contains("api ready"));
    }

    #[test]
    fn renders_footer_message_when_present() {
        let mut tui = TuiApp::new(2);
        tui.set_footer_message("All servers must be running");
        let state = sample_state();

        let output = render_to_string(&tui, &state);

        assert!(output.contains("All servers must be running"));
    }
}
