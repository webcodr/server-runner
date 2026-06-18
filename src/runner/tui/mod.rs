use anyhow::Context;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use std::io::{Stdout, stdout};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::Config;
use crate::core::actor::{EngineCommand, run_tui_engine};
use crate::core::state::{AppState, ServerStatus};
use crate::runner::tui::app::TuiApp;
use crate::runner::tui::input::{InputEvent, UiAction, map_event};

pub mod app;
pub mod input;
pub mod ui;

pub struct TerminalMode {
    pub mouse_capture: bool,
}

impl TerminalMode {
    pub fn new(mouse_capture: bool) -> Self {
        Self { mouse_capture }
    }
}

struct TerminalGuard {
    mode: TerminalMode,
}

impl TerminalGuard {
    fn enter(mode: TerminalMode) -> anyhow::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        if let Err(error) = enter_terminal_mode(&mut stdout, &mode) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }

        Ok(Self { mode })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal_mode(&self.mode);
    }
}

fn enter_terminal_mode(stdout: &mut Stdout, mode: &TerminalMode) -> std::io::Result<()> {
    execute!(stdout, EnterAlternateScreen)?;
    if mode.mouse_capture {
        execute!(stdout, EnableMouseCapture)?;
    }
    Ok(())
}

fn restore_terminal_mode(mode: &TerminalMode) {
    let mut stdout = stdout();
    if mode.mouse_capture {
        let _ = execute!(stdout, DisableMouseCapture);
    }
    let _ = execute!(stdout, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

fn install_panic_hook() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        restore_terminal_mode(&TerminalMode::new(true));
        previous_hook(panic_info);
    }));
}

pub async fn run(config: Config, max_attempts: u8) -> anyhow::Result<()> {
    install_panic_hook();

    let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));
    let mut app = TuiApp::new(config.servers.len());
    let (tx, rx) = mpsc::channel(32);
    let actor_state = Arc::clone(&state);
    let actor = tokio::spawn(run_tui_engine(config, max_attempts, actor_state, rx));

    let ui_result = run_terminal_loop(&mut app, Arc::clone(&state), tx.clone()).await;
    let _ = tx.send(EngineCommand::Quit).await;
    let actor_result = actor.await.context("TUI engine task failed")?;

    ui_result?;
    actor_result?;
    Ok(())
}

async fn run_terminal_loop(
    app: &mut TuiApp,
    state: Arc<Mutex<AppState>>,
    tx: mpsc::Sender<EngineCommand>,
) -> anyhow::Result<()> {
    let _guard = TerminalGuard::enter(TerminalMode::new(true))?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    while !app.should_quit() {
        terminal.draw(|frame| ui::render(frame, app, &state))?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    handle_action(map_event(InputEvent::Key(key)), app, &state, &tx).await?;
                }
                Event::Mouse(mouse) => {
                    handle_action(map_event(InputEvent::Mouse(mouse)), app, &state, &tx).await?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

async fn handle_action(
    action: UiAction,
    app: &mut TuiApp,
    state: &Arc<Mutex<AppState>>,
    tx: &mpsc::Sender<EngineCommand>,
) -> anyhow::Result<()> {
    match action {
        UiAction::Restart => {
            if app.is_final_row() {
                app.set_footer_message("Restart applies to servers only");
            } else {
                tx.send(EngineCommand::Restart(app.selected())).await?;
            }
        }
        UiAction::StopStart => {
            if app.is_final_row() {
                app.set_footer_message("Stop/start applies to servers only");
            } else {
                tx.send(EngineCommand::StopStart(app.selected())).await?;
            }
        }
        UiAction::RerunFinalCommand => {
            if !app.is_final_row() {
                app.set_footer_message("Re-run applies to the final command only");
            } else if all_servers_running(state) {
                tx.send(EngineCommand::RerunFinalCommand).await?;
            } else {
                app.set_footer_message("All servers must be running before re-run");
            }
        }
        UiAction::Quit => {
            let _ = tx.send(EngineCommand::Quit).await;
            app.request_quit();
        }
        other => app.apply_action(other),
    }

    Ok(())
}

fn all_servers_running(state: &Arc<Mutex<AppState>>) -> bool {
    state
        .lock()
        .map(|state| {
            state
                .servers
                .iter()
                .all(|server| server.status == ServerStatus::Running)
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::error::TryRecvError;

    #[test]
    fn terminal_guard_mode_tracks_mouse_capture_setting() {
        let mode = TerminalMode::new(true);
        assert!(mode.mouse_capture);
    }

    #[tokio::test]
    async fn rerun_final_command_on_server_row_sets_footer_without_command() {
        let servers = vec![crate::config::Server {
            name: "API".to_string(),
            url: "http://127.0.0.1:3000".to_string(),
            command: "python3 -m http.server 3000".to_string(),
            timeout: 5,
        }];
        let state = Arc::new(Mutex::new(AppState::new(&servers, "npm test")));
        state.lock().unwrap().servers[0].status = ServerStatus::Running;
        let mut app = TuiApp::new(1);
        let (tx, mut rx) = mpsc::channel(1);

        handle_action(UiAction::RerunFinalCommand, &mut app, &state, &tx)
            .await
            .unwrap();

        assert_eq!(
            app.footer_message(),
            Some("Re-run applies to the final command only")
        );
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    }
}
