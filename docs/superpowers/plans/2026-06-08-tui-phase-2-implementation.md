# TUI Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the full opt-in `--tui` control panel with live status/logs, keyboard controls, mouse-wheel log scrolling, and safe cleanup while preserving plain mode.

**Architecture:** Add a TUI-specific engine actor that owns process orchestration and mutates shared `AppState`, while the TUI runner renders that state and sends commands over a channel. Keep `runner::plain` on its current verified path; only `main.rs` dispatch changes for `--tui`.

**Tech Stack:** Rust 2024, tokio, ratatui, crossterm, command-group, reqwest async client, assert_cmd/predicates for integration tests.

---

## File Structure

- Modify `Cargo.toml` / `Cargo.lock`: add `ratatui` and `crossterm`.
- Modify `src/core/mod.rs`: export the new `actor` module.
- Create `src/core/actor.rs`: TUI engine actor, `EngineCommand`, process ownership, polling, restart/stop-start/rerun/quit behavior, actor unit tests.
- Modify `src/core/server.rs`: add TUI-friendly spawn helpers that can capture output without teeing to the plain terminal.
- Modify `src/core/command.rs`: add TUI-friendly final-command spawn that exposes completion and log capture without terminal teeing.
- Modify `src/core/state.rs`: add small constructors/helpers for `AppState`, status updates, and line access used by actor/rendering tests.
- Create `src/runner/tui/mod.rs`: `run`, terminal guard, event loop, actor task startup/shutdown.
- Create `src/runner/tui/app.rs`: UI-local selection, scroll, tail-following, footer message behavior.
- Create `src/runner/tui/input.rs`: keyboard/mouse event mapping into UI actions and engine commands.
- Create `src/runner/tui/ui.rs`: ratatui rendering and `TestBackend` tests.
- Modify `src/runner/mod.rs`: export `tui`.
- Modify `src/main.rs`: dispatch `--tui` to `runner::tui::run`.
- Modify `README.md`: document the new `--tui` option after implementation.

---

## Task 1: Add TUI Dependencies

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Add dependencies**

Edit `Cargo.toml` dependencies:

```toml
crossterm = "0.28"
ratatui = "0.29"
```

Keep existing dependencies unchanged.

- [ ] **Step 2: Verify dependency resolution**

Run: `cargo check`

Expected: PASS. `Cargo.lock` updates with ratatui/crossterm transitive dependencies.

- [ ] **Step 3: Verify plain mode still compiles and tests still pass**

Run: `cargo test`

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add TUI dependencies"
```

---

## Task 2: Add AppState Constructors And Access Helpers

**Files:**
- Modify: `src/core/state.rs`

This task adds small, pure helpers so subsequent actor and UI code does not duplicate AppState setup logic.

- [ ] **Step 1: Write failing tests**

Append these tests to the existing `#[cfg(test)] mod app_state_tests` in `src/core/state.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test app_state_`

Expected: FAIL because `AppState::new`, `selectable_len`, and `is_final_selection` do not exist.

- [ ] **Step 3: Implement helpers**

In `src/core/state.rs`, add import:

```rust
use crate::config::Server;
use crate::core::server::LOG_CAPACITY;
```

Add implementations after the `AppState` struct:

```rust
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
```

Remove the broad `#[allow(dead_code)]` annotations from `FinalCmdStatus`, `ServerView`, `FinalCmdView`, and `AppState`. Keep narrow allows only on individual fields, variants, or methods that still need them before later tasks consume them.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test app_state_`

Expected: PASS.

- [ ] **Step 5: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run the check.

- [ ] **Step 6: Commit**

```bash
git add src/core/state.rs
git commit -m "feat: add AppState constructors"
```

---

## Task 3: Add TUI App Selection And Scroll State

**Files:**
- Create: `src/runner/tui/mod.rs`
- Create: `src/runner/tui/app.rs`
- Modify: `src/runner/mod.rs`

- [ ] **Step 1: Create module shell**

Create `src/runner/tui/mod.rs`:

```rust
pub mod app;
```

Modify `src/runner/mod.rs`:

```rust
pub mod plain;
pub mod tui;
```

- [ ] **Step 2: Write failing tests**

Create `src/runner/tui/app.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_wraps_across_servers_and_final_command() {
        let mut app = TuiApp::new(3);

        assert_eq!(app.selected(), 0);
        app.select_next();
        app.select_next();
        app.select_next();
        assert_eq!(app.selected(), 3);
        app.select_next();
        assert_eq!(app.selected(), 0);
        app.select_previous();
        assert_eq!(app.selected(), 3);
    }

    #[test]
    fn scrolling_disables_tail_follow_until_end() {
        let mut app = TuiApp::new(1);

        assert!(app.follows_tail());
        app.scroll_up(3);
        assert_eq!(app.scroll_offset(), 3);
        assert!(!app.follows_tail());
        app.scroll_down(1);
        assert_eq!(app.scroll_offset(), 2);
        assert!(!app.follows_tail());
        app.follow_tail();
        assert_eq!(app.scroll_offset(), 0);
        assert!(app.follows_tail());
    }

    #[test]
    fn footer_message_can_be_set_and_cleared() {
        let mut app = TuiApp::new(1);

        app.set_footer_message("Servers are not ready");
        assert_eq!(app.footer_message(), Some("Servers are not ready"));
        app.clear_footer_message();
        assert_eq!(app.footer_message(), None);
    }
}
```

- [ ] **Step 3: Run tests to verify RED**

Run: `cargo test runner::tui::app`

Expected: FAIL because `TuiApp` does not exist.

- [ ] **Step 4: Implement TuiApp**

Add this above the test module in `src/runner/tui/app.rs`:

```rust
pub struct TuiApp {
    selected: usize,
    scroll: Vec<usize>,
    follow_tail: Vec<bool>,
    footer_message: Option<String>,
    should_quit: bool,
}

impl TuiApp {
    pub fn new(server_count: usize) -> Self {
        let row_count = server_count + 1;
        Self {
            selected: 0,
            scroll: vec![0; row_count],
            follow_tail: vec![true; row_count],
            footer_message: None,
            should_quit: false,
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn request_quit(&mut self) {
        self.should_quit = true;
    }

    pub fn select_next(&mut self) {
        if self.scroll.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.scroll.len();
    }

    pub fn select_previous(&mut self) {
        if self.scroll.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.scroll.len() - 1
        } else {
            self.selected - 1
        };
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll[self.selected] = self.scroll[self.selected].saturating_add(amount);
        self.follow_tail[self.selected] = false;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll[self.selected] = self.scroll[self.selected].saturating_sub(amount);
        if self.scroll[self.selected] == 0 {
            self.follow_tail[self.selected] = true;
        }
    }

    pub fn follow_tail(&mut self) {
        self.scroll[self.selected] = 0;
        self.follow_tail[self.selected] = true;
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll[self.selected]
    }

    pub fn follows_tail(&self) -> bool {
        self.follow_tail[self.selected]
    }

    pub fn set_footer_message(&mut self, message: impl Into<String>) {
        self.footer_message = Some(message.into());
    }

    pub fn clear_footer_message(&mut self) {
        self.footer_message = None;
    }

    pub fn footer_message(&self) -> Option<&str> {
        self.footer_message.as_deref()
    }
}
```

- [ ] **Step 5: Run tests to verify GREEN**

Run: `cargo test runner::tui::app`

Expected: PASS.

- [ ] **Step 6: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 7: Commit**

```bash
git add src/runner/mod.rs src/runner/tui/mod.rs src/runner/tui/app.rs
git commit -m "feat: add TUI app state"
```

---

## Task 4: Add Input Mapping For Keyboard And Mouse Wheel

**Files:**
- Create: `src/runner/tui/input.rs`
- Modify: `src/runner/tui/mod.rs`
- Modify: `src/runner/tui/app.rs`

- [ ] **Step 1: Export input module**

Modify `src/runner/tui/mod.rs`:

```rust
pub mod app;
pub mod input;
```

- [ ] **Step 2: Write failing tests**

Create `src/runner/tui/input.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    use super::*;

    fn key(code: KeyCode) -> InputEvent {
        InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn maps_navigation_keys() {
        assert_eq!(map_event(key(KeyCode::Down)), UiAction::SelectNext);
        assert_eq!(map_event(key(KeyCode::Char('j'))), UiAction::SelectNext);
        assert_eq!(map_event(key(KeyCode::Up)), UiAction::SelectPrevious);
        assert_eq!(map_event(key(KeyCode::Char('k'))), UiAction::SelectPrevious);
        assert_eq!(map_event(key(KeyCode::Tab)), UiAction::SelectNext);
    }

    #[test]
    fn maps_scroll_keys_and_mouse_wheel() {
        assert_eq!(map_event(key(KeyCode::PageUp)), UiAction::ScrollUp(10));
        assert_eq!(map_event(key(KeyCode::PageDown)), UiAction::ScrollDown(10));
        assert_eq!(map_event(key(KeyCode::End)), UiAction::FollowTail);

        assert_eq!(
            map_event(InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 10,
                row: 10,
                modifiers: KeyModifiers::NONE,
            })),
            UiAction::ScrollUp(3)
        );
        assert_eq!(
            map_event(InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 10,
                row: 10,
                modifiers: KeyModifiers::NONE,
            })),
            UiAction::ScrollDown(3)
        );
    }

    #[test]
    fn maps_engine_commands_and_quit() {
        assert_eq!(map_event(key(KeyCode::Char('r'))), UiAction::Restart);
        assert_eq!(map_event(key(KeyCode::Char('s'))), UiAction::StopStart);
        assert_eq!(map_event(key(KeyCode::Char('e'))), UiAction::RerunFinalCommand);
        assert_eq!(map_event(key(KeyCode::Char('q'))), UiAction::Quit);
        assert_eq!(
            map_event(InputEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))),
            UiAction::Quit
        );

        assert_eq!(
            map_event(InputEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 1,
                row: 1,
                modifiers: KeyModifiers::NONE,
            })),
            UiAction::None
        );
    }
}
```

- [ ] **Step 3: Run tests to verify RED**

Run: `cargo test runner::tui::input`

Expected: FAIL because `InputEvent`, `UiAction`, and `map_event` do not exist.

- [ ] **Step 4: Implement input mapping**

Add this above the tests in `src/runner/tui/input.rs`:

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    SelectNext,
    SelectPrevious,
    ScrollUp(usize),
    ScrollDown(usize),
    FollowTail,
    Restart,
    StopStart,
    RerunFinalCommand,
    Quit,
    None,
}

pub fn map_event(event: InputEvent) -> UiAction {
    match event {
        InputEvent::Key(key) => map_key(key),
        InputEvent::Mouse(mouse) => map_mouse(mouse),
    }
}

fn map_key(key: KeyEvent) -> UiAction {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => UiAction::Quit,
        (KeyCode::Char('q'), _) => UiAction::Quit,
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) | (KeyCode::Tab, _) => UiAction::SelectNext,
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => UiAction::SelectPrevious,
        (KeyCode::PageUp, _) => UiAction::ScrollUp(10),
        (KeyCode::PageDown, _) => UiAction::ScrollDown(10),
        (KeyCode::End, _) => UiAction::FollowTail,
        (KeyCode::Char('r'), _) => UiAction::Restart,
        (KeyCode::Char('s'), _) => UiAction::StopStart,
        (KeyCode::Char('e'), _) => UiAction::RerunFinalCommand,
        _ => UiAction::None,
    }
}

fn map_mouse(mouse: MouseEvent) -> UiAction {
    match mouse.kind {
        MouseEventKind::ScrollUp => UiAction::ScrollUp(3),
        MouseEventKind::ScrollDown => UiAction::ScrollDown(3),
        _ => UiAction::None,
    }
}
```

- [ ] **Step 5: Add action application tests**

Append to `src/runner/tui/app.rs` test module:

```rust
#[test]
fn applies_input_actions_to_local_state() {
    let mut app = TuiApp::new(2);

    app.apply_action(crate::runner::tui::input::UiAction::SelectNext);
    assert_eq!(app.selected(), 1);
    app.apply_action(crate::runner::tui::input::UiAction::ScrollUp(3));
    assert_eq!(app.scroll_offset(), 3);
    app.apply_action(crate::runner::tui::input::UiAction::FollowTail);
    assert_eq!(app.scroll_offset(), 0);
    app.apply_action(crate::runner::tui::input::UiAction::Quit);
    assert!(app.should_quit());
}
```

- [ ] **Step 6: Implement action application**

Add import at top of `src/runner/tui/app.rs`:

```rust
use crate::runner::tui::input::UiAction;
```

Add method inside `impl TuiApp`:

```rust
pub fn apply_action(&mut self, action: UiAction) {
    match action {
        UiAction::SelectNext => self.select_next(),
        UiAction::SelectPrevious => self.select_previous(),
        UiAction::ScrollUp(amount) => self.scroll_up(amount),
        UiAction::ScrollDown(amount) => self.scroll_down(amount),
        UiAction::FollowTail => self.follow_tail(),
        UiAction::Quit => self.request_quit(),
        UiAction::Restart | UiAction::StopStart | UiAction::RerunFinalCommand | UiAction::None => {}
    }
}
```

- [ ] **Step 7: Run tests to verify GREEN**

Run: `cargo test runner::tui`

Expected: PASS.

- [ ] **Step 8: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 9: Commit**

```bash
git add src/runner/tui/mod.rs src/runner/tui/input.rs src/runner/tui/app.rs
git commit -m "feat: map TUI keyboard and mouse input"
```

---

## Task 5: Render The TUI With TestBackend

**Files:**
- Create: `src/runner/tui/ui.rs`
- Modify: `src/runner/tui/mod.rs`
- Modify: `src/runner/tui/app.rs`

- [ ] **Step 1: Export ui module**

Modify `src/runner/tui/mod.rs`:

```rust
pub mod app;
pub mod input;
pub mod ui;
```

- [ ] **Step 2: Write failing render tests**

Create `src/runner/tui/ui.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    use std::sync::{Arc, Mutex};

    use crate::core::state::{AppState, Attempts, FinalCmdStatus, RingBuffer, ServerStatus};
    use crate::runner::tui::app::TuiApp;

    use super::*;

    fn sample_state() -> Arc<Mutex<AppState>> {
        let mut app_state = AppState {
            servers: vec![
                crate::core::state::ServerView {
                    name: "API".to_string(),
                    url: "http://127.0.0.1:3000".to_string(),
                    status: ServerStatus::Running,
                    attempts: Attempts(2),
                    log: Arc::new(Mutex::new(RingBuffer::new(10))),
                },
                crate::core::state::ServerView {
                    name: "Worker".to_string(),
                    url: "http://127.0.0.1:3001".to_string(),
                    status: ServerStatus::Failed,
                    attempts: Attempts(5),
                    log: Arc::new(Mutex::new(RingBuffer::new(10))),
                },
            ],
            final_cmd: crate::core::state::FinalCmdView {
                command: "npm test".to_string(),
                status: FinalCmdStatus::Succeeded(0),
                log: Arc::new(Mutex::new(RingBuffer::new(10))),
            },
        };

        app_state.servers[0].log.lock().unwrap().push("api ready".to_string());
        app_state.servers[1].log.lock().unwrap().push("worker failed".to_string());
        app_state.final_cmd.log.lock().unwrap().push("tests passed".to_string());

        Arc::new(Mutex::new(app_state))
    }

    #[test]
    fn renders_sidebar_statuses_and_selected_log() {
        let state = sample_state();
        let tui = TuiApp::new(2);
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(frame, &tui, &state)).unwrap();
        let contents = buffer_to_string(terminal.backend().buffer());

        assert!(contents.contains("server-runner"));
        assert!(contents.contains("API"));
        assert!(contents.contains("RUN"));
        assert!(contents.contains("Worker"));
        assert!(contents.contains("FAIL"));
        assert!(contents.contains("npm test"));
        assert!(contents.contains("OK 0"));
        assert!(contents.contains("api ready"));
    }

    #[test]
    fn renders_footer_message_when_present() {
        let state = sample_state();
        let mut tui = TuiApp::new(2);
        tui.set_footer_message("All servers must be running");
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(frame, &tui, &state)).unwrap();
        let contents = buffer_to_string(terminal.backend().buffer());

        assert!(contents.contains("All servers must be running"));
    }

    fn buffer_to_string(buffer: &Buffer) -> String {
        let mut output = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                output.push_str(buffer.get(x, y).symbol());
            }
            output.push('\n');
        }
        output
    }
}
```

- [ ] **Step 3: Run tests to verify RED**

Run: `cargo test runner::tui::ui`

Expected: FAIL because `render` does not exist.

- [ ] **Step 4: Implement rendering**

Add this above tests in `src/runner/tui/ui.rs`:

```rust
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use std::sync::{Arc, Mutex};

use crate::core::state::{AppState, FinalCmdStatus, ServerStatus};
use crate::runner::tui::app::TuiApp;

pub fn render(frame: &mut Frame, tui: &TuiApp, state: &Arc<Mutex<AppState>>) {
    let Ok(app_state) = state.lock() else {
        return;
    };

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(frame.area());
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(10)])
        .split(root[0]);

    let sidebar_items = sidebar_items(tui, &app_state);
    let sidebar = List::new(sidebar_items).block(Block::default().title("Servers").borders(Borders::ALL));
    frame.render_widget(sidebar, body[0]);

    let (title, lines) = selected_log(tui, &app_state);
    let logs = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(logs, body[1]);

    let footer = tui.footer_message().unwrap_or("↑/↓ select  PgUp/PgDn scroll  r restart  s stop/start  e rerun  q quit");
    frame.render_widget(Paragraph::new(footer), root[1]);
}

fn sidebar_items(tui: &TuiApp, app_state: &AppState) -> Vec<ListItem<'static>> {
    let mut items = Vec::new();
    for (idx, server) in app_state.servers.iter().enumerate() {
        let selected = if tui.selected() == idx { "▶ " } else { "  " };
        let (glyph, status, color) = server_status(server.status, server.attempts.0);
        items.push(ListItem::new(Line::from(vec![
            Span::raw(selected.to_string()),
            Span::styled(glyph, Style::default().fg(color)),
            Span::raw(format!(" {:<12} {}", server.name, status)),
        ])));
    }

    let final_idx = app_state.servers.len();
    let selected = if tui.selected() == final_idx { "▶ " } else { "  " };
    let status = final_status(app_state.final_cmd.status);
    items.push(ListItem::new(Line::from(vec![
        Span::raw(selected.to_string()),
        Span::styled("⚙", Style::default().fg(Color::Cyan)),
        Span::raw(format!(" {:<12} {}", app_state.final_cmd.command, status)),
    ])));

    items
}

fn server_status(status: ServerStatus, attempts: u8) -> (&'static str, String, Color) {
    match status {
        ServerStatus::Waiting => ("◐", format!("WAIT {attempts}"), Color::Yellow),
        ServerStatus::Running => ("●", "RUN".to_string(), Color::Green),
        ServerStatus::Failed => ("✗", "FAIL".to_string(), Color::Red),
        ServerStatus::Stopped => ("○", "STOP".to_string(), Color::DarkGray),
    }
}

fn final_status(status: FinalCmdStatus) -> String {
    match status {
        FinalCmdStatus::Idle => "IDLE".to_string(),
        FinalCmdStatus::Running => "RUN".to_string(),
        FinalCmdStatus::Succeeded(code) => format!("OK {code}"),
        FinalCmdStatus::Failed(code) => format!("ERR {code}"),
    }
}

fn selected_log(tui: &TuiApp, app_state: &AppState) -> (String, Vec<Line<'static>>) {
    if app_state.is_final_selection(tui.selected()) {
        let lines = app_state
            .final_cmd
            .log
            .lock()
            .map(|log| log.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        return (format!("Logs: {}", app_state.final_cmd.command), log_lines(lines, tui.scroll_offset()));
    }

    let Some(server) = app_state.servers.get(tui.selected()) else {
        return ("Logs".to_string(), Vec::new());
    };
    let lines = server
        .log
        .lock()
        .map(|log| log.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    (format!("Logs: {}", server.name), log_lines(lines, tui.scroll_offset()))
}

fn log_lines(mut lines: Vec<String>, scroll_offset: usize) -> Vec<Line<'static>> {
    if scroll_offset > 0 && scroll_offset < lines.len() {
        let keep = lines.len().saturating_sub(scroll_offset);
        lines.truncate(keep);
    }
    lines.into_iter().map(Line::from).collect()
}
```

After adding the render implementation, remove any imports that are not used by the final code in `src/runner/tui/ui.rs`.

- [ ] **Step 5: Run tests to verify GREEN**

Run: `cargo test runner::tui::ui`

Expected: PASS.

- [ ] **Step 6: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 7: Commit**

```bash
git add src/runner/tui/mod.rs src/runner/tui/ui.rs
git commit -m "feat: render TUI control panel"
```

---

## Task 6: Add TUI Process Spawning Without Terminal Tee

**Files:**
- Modify: `src/core/server.rs`
- Modify: `src/core/command.rs`

The existing plain-mode spawn helpers tee process output to stdout/stderr. The TUI must capture output into logs without writing behind the alternate screen.

- [ ] **Step 1: Write failing tests for captured output without tee**

Add to `src/core/command.rs` tests:

```rust
#[cfg(test)]
mod tui_tests {
    use super::*;

    #[tokio::test]
    async fn spawn_captured_records_output_lines() {
        let mut command = spawn_captured("sh -c 'echo out; echo err >&2'").unwrap();
        let status = command.child.wait().await.unwrap();
        for reader in command.readers {
            let _ = reader.await;
        }

        assert!(status.success());
        let lines: Vec<_> = command.log.lock().unwrap().iter().cloned().collect();
        assert!(lines.contains(&"out".to_string()));
        assert!(lines.contains(&"err".to_string()));
    }
}
```

Add to `src/core/server.rs` tests:

```rust
#[cfg(test)]
mod tui_tests {
    use super::*;

    #[tokio::test]
    async fn spawn_captured_server_records_output_lines() {
        let mut server = ServerProcess::spawn_captured("Test", "sh -c 'echo server-out; sleep 5'").unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        server.stop().await.unwrap();

        let lines: Vec<_> = server.log.lock().unwrap().iter().cloned().collect();
        assert!(lines.contains(&"server-out".to_string()));
    }
}
```

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test spawn_captured`

Expected: FAIL because `spawn_captured` and `ServerProcess::spawn_captured` do not exist.

- [ ] **Step 3: Refactor output readers to support tee flag**

In `src/core/server.rs`, change `ServerProcess::spawn` to delegate:

```rust
pub fn spawn(name: &str, command: &str) -> anyhow::Result<Self> {
    Self::spawn_inner(name, command, true)
}

pub fn spawn_captured(name: &str, command: &str) -> anyhow::Result<Self> {
    Self::spawn_inner(name, command, false)
}

fn spawn_inner(name: &str, command: &str, tee_output: bool) -> anyhow::Result<Self> {
    let mut cmd = build_command(command)?;
    let mut child = cmd.group_spawn()?;
    let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));

    if let Some(stdout) = child.inner().stdout.take() {
        spawn_reader(stdout, Arc::clone(&log), false, tee_output);
    }
    if let Some(stderr) = child.inner().stderr.take() {
        spawn_reader(stderr, Arc::clone(&log), true, tee_output);
    }

    Ok(Self {
        name: name.to_string(),
        log,
        child,
    })
}
```

Update `spawn_reader` signature and output call:

```rust
fn spawn_reader<R>(mut stream: R, log: Arc<Mutex<RingBuffer>>, stderr: bool, tee_output: bool)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = [0; 8192];
        let mut line = String::new();

        while let Ok(n) = stream.read(&mut buf).await {
            if n == 0 {
                break;
            }

            if tee_output {
                write_output(&buf[..n], stderr);
            }
            capture_lines(&buf[..n], &mut line, &log);
        }

        if !line.is_empty()
            && let Ok(mut buf) = log.lock()
        {
            buf.push(std::mem::take(&mut line));
        }
    });
}
```

In `src/core/command.rs`, add `spawn_captured` and delegate through `spawn_inner`:

```rust
pub fn spawn(command: &str) -> anyhow::Result<FinalCommand> {
    spawn_inner(command, true)
}

pub fn spawn_captured(command: &str) -> anyhow::Result<FinalCommand> {
    spawn_inner(command, false)
}

fn spawn_inner(command: &str, tee_output: bool) -> anyhow::Result<FinalCommand> {
    let mut cmd = build_command(command)?;
    if !tee_output {
        cmd.kill_on_drop(true);
    }
    let mut child = cmd.spawn()?;
    let log = Arc::new(Mutex::new(RingBuffer::new(LOG_CAPACITY)));
    let mut readers = Vec::new();

    if let Some(stdout) = child.stdout.take() {
        readers.push(spawn_reader(stdout, Arc::clone(&log), false, tee_output));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(spawn_reader(stderr, Arc::clone(&log), true, tee_output));
    }

    Ok(FinalCommand { child, log, readers })
}
```

Update command `spawn_reader` exactly like the server version but returning `JoinHandle<()>`.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test spawn_captured`

Expected: PASS.

- [ ] **Step 5: Verify plain output preservation test still passes**

Run: `cargo test preserves_final_command_stdout_and_stderr -- --nocapture`

Expected: PASS.

- [ ] **Step 6: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 7: Commit**

```bash
git add src/core/server.rs src/core/command.rs
git commit -m "feat: capture process output without terminal tee"
```

---

## Task 7: Add TUI Engine Actor Startup, Polling, And Failure State

**Files:**
- Create: `src/core/actor.rs`
- Modify: `src/core/mod.rs`
- Modify: `src/core/state.rs`

- [ ] **Step 1: Export actor module**

Modify `src/core/mod.rs`:

```rust
pub mod actor;
pub mod command;
pub mod health;
pub mod server;
pub mod state;
```

- [ ] **Step 2: Write actor state tests**

Create `src/core/actor.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_running_detects_only_running_servers() {
        let config = crate::config::Config {
            servers: vec![crate::config::Server {
                name: "A".to_string(),
                url: "http://127.0.0.1:1".to_string(),
                command: "echo a".to_string(),
                timeout: 1,
            }],
            command: "echo done".to_string(),
        };
        let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

        assert!(!all_servers_running(&state));
        state.lock().unwrap().servers[0].status = ServerStatus::Running;
        assert!(all_servers_running(&state));
    }

    #[test]
    fn marks_failed_when_attempts_exhausted() {
        let config = crate::config::Config {
            servers: vec![crate::config::Server {
                name: "A".to_string(),
                url: "http://127.0.0.1:1".to_string(),
                command: "echo a".to_string(),
                timeout: 1,
            }],
            command: "echo done".to_string(),
        };
        let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

        increment_attempt_or_fail(&state, 0, 1);

        let guard = state.lock().unwrap();
        assert_eq!(guard.servers[0].attempts, 1u8);
        assert_eq!(guard.servers[0].status, ServerStatus::Failed);
    }
}
```

- [ ] **Step 3: Run tests to verify RED**

Run: `cargo test core::actor`

Expected: FAIL because actor types and helpers do not exist.

- [ ] **Step 4: Implement actor skeleton and helpers**

Add this above tests in `src/core/actor.rs`:

```rust
use tokio::sync::mpsc;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::Config;
use crate::core::server::ServerProcess;
use crate::core::state::{AppState, FinalCmdStatus, ServerStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineCommand {
    Restart(usize),
    StopStart(usize),
    RerunFinalCommand,
    Quit,
}

pub async fn run_tui_engine(
    config: Config,
    max_attempts: u8,
    state: Arc<Mutex<AppState>>,
    mut commands: mpsc::Receiver<EngineCommand>,
) -> anyhow::Result<()> {
    let mut processes = Vec::with_capacity(config.servers.len());
    for server in &config.servers {
        processes.push(Some(ServerProcess::spawn_captured(&server.name, &server.command)?));
    }

    let mut final_started = false;
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                poll_servers(&config, max_attempts, &state).await?;
                if !final_started && all_servers_running(&state) {
                    set_final_status(&state, FinalCmdStatus::Running);
                    final_started = true;
                }
            }
            command = commands.recv() => {
                match command {
                    Some(EngineCommand::Quit) | None => {
                        stop_all(&mut processes).await?;
                        return Ok(());
                    }
                    Some(EngineCommand::Restart(_))
                    | Some(EngineCommand::StopStart(_))
                    | Some(EngineCommand::RerunFinalCommand) => {}
                }
            }
        }
    }
}

async fn poll_servers(
    config: &Config,
    max_attempts: u8,
    state: &Arc<Mutex<AppState>>,
) -> anyhow::Result<()> {
    for (idx, server) in config.servers.iter().enumerate() {
        let status = {
            let guard = state.lock().unwrap();
            guard.servers[idx].status
        };
        if status != ServerStatus::Waiting {
            continue;
        }

        increment_attempt_or_fail(state, idx, max_attempts);
        if state.lock().unwrap().servers[idx].status == ServerStatus::Failed {
            continue;
        }

        if crate::core::health::check(&server.name, &server.url, server.timeout).await? == ServerStatus::Running {
            state.lock().unwrap().servers[idx].status = ServerStatus::Running;
        }
    }
    Ok(())
}

fn increment_attempt_or_fail(state: &Arc<Mutex<AppState>>, idx: usize, max_attempts: u8) {
    let mut guard = state.lock().unwrap();
    let server = &mut guard.servers[idx];
    server.attempts += 1;
    if server.attempts.0 >= max_attempts {
        server.status = ServerStatus::Failed;
        if let Ok(mut log) = server.log.lock() {
            log.push(format!("Failed after {} attempts", server.attempts));
        }
    }
}

fn all_servers_running(state: &Arc<Mutex<AppState>>) -> bool {
    state
        .lock()
        .map(|state| state.servers.iter().all(|server| server.status == ServerStatus::Running))
        .unwrap_or(false)
}

fn set_final_status(state: &Arc<Mutex<AppState>>, status: FinalCmdStatus) {
    if let Ok(mut state) = state.lock() {
        state.final_cmd.status = status;
    }
}

async fn stop_all(processes: &mut [Option<ServerProcess>]) -> anyhow::Result<()> {
    for process in processes.iter_mut().flatten() {
        process.stop().await?;
    }
    Ok(())
}
```

This actor skeleton does not yet execute final commands or restart/stop-start. Later tasks add that behavior.

- [ ] **Step 5: Run tests to verify GREEN**

Run: `cargo test core::actor`

Expected: PASS.

- [ ] **Step 6: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 7: Commit**

```bash
git add src/core/mod.rs src/core/actor.rs
git commit -m "feat: add TUI engine actor skeleton"
```

---

## Task 8: Implement Final Command Lifecycle In Actor

**Files:**
- Modify: `src/core/actor.rs`
- Modify: `src/core/command.rs` if final-command kill support is needed

- [ ] **Step 1: Write failing tests for final command status transitions**

Append to `src/core/actor.rs` tests:

```rust
#[tokio::test]
async fn run_final_command_updates_success_status_and_log() {
    let config = crate::config::Config {
        servers: Vec::new(),
        command: "sh -c 'echo final-ok'".to_string(),
    };
    let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

    run_final_command_for_tui(&config.command, &state).await.unwrap();

    let guard = state.lock().unwrap();
    assert_eq!(guard.final_cmd.status, FinalCmdStatus::Succeeded(0));
    let lines: Vec<_> = guard.final_cmd.log.lock().unwrap().iter().cloned().collect();
    assert!(lines.contains(&"final-ok".to_string()));
}

#[tokio::test]
async fn run_final_command_updates_failed_status() {
    let config = crate::config::Config {
        servers: Vec::new(),
        command: "sh -c 'exit 7'".to_string(),
    };
    let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

    run_final_command_for_tui(&config.command, &state).await.unwrap();

    assert_eq!(state.lock().unwrap().final_cmd.status, FinalCmdStatus::Failed(7));
}
```

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test run_final_command_updates`

Expected: FAIL because `run_final_command_for_tui` does not exist.

- [ ] **Step 3: Implement final command runner helper**

Add to `src/core/actor.rs`:

```rust
async fn run_final_command_for_tui(
    command: &str,
    state: &Arc<Mutex<AppState>>,
) -> anyhow::Result<()> {
    set_final_status(state, FinalCmdStatus::Running);
    let final_cmd = crate::core::command::spawn_captured(command)?;
    let log = Arc::clone(&final_cmd.log);
    if let Ok(mut guard) = state.lock() {
        guard.final_cmd.log = Arc::clone(&log);
    }

    let mut child = final_cmd.child;
    let status = child.wait().await?;
    for reader in final_cmd.readers {
        let _ = reader.await;
    }

    let code = status.code().unwrap_or(1);
    if status.success() {
        set_final_status(state, FinalCmdStatus::Succeeded(code));
    } else {
        set_final_status(state, FinalCmdStatus::Failed(code));
    }
    Ok(())
}
```

Update `run_tui_engine`: when all servers are running for the first time, call `run_final_command_for_tui(&config.command, &state).await?` instead of only setting `FinalCmdStatus::Running`. This blocks the actor while the command runs for this intermediate task; Task 12 makes final-command execution responsive to quit and rerun commands before the TUI is considered complete.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test run_final_command_updates`

Expected: PASS.

- [ ] **Step 5: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 6: Commit**

```bash
git add src/core/actor.rs
git commit -m "feat: run final command in TUI actor"
```

---

## Task 9: Implement Restart And Stop/Start Commands

**Files:**
- Modify: `src/core/actor.rs`

- [ ] **Step 1: Write failing tests for restart and stop/start state helpers**

Append to `src/core/actor.rs` tests:

```rust
#[test]
fn restart_resets_server_state() {
    let config = crate::config::Config {
        servers: vec![crate::config::Server {
            name: "A".to_string(),
            url: "http://127.0.0.1:1".to_string(),
            command: "echo a".to_string(),
            timeout: 1,
        }],
        command: "echo done".to_string(),
    };
    let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));
    {
        let mut guard = state.lock().unwrap();
        guard.servers[0].status = ServerStatus::Failed;
        guard.servers[0].attempts = crate::core::state::Attempts(5);
    }

    reset_server_for_start(&state, 0, "Restarting server");

    let guard = state.lock().unwrap();
    assert_eq!(guard.servers[0].status, ServerStatus::Waiting);
    assert_eq!(guard.servers[0].attempts, 0u8);
    let lines: Vec<_> = guard.servers[0].log.lock().unwrap().iter().cloned().collect();
    assert!(lines.contains(&"--- Restarting server ---".to_string()));
}

#[test]
fn stop_marks_server_stopped() {
    let config = crate::config::Config {
        servers: vec![crate::config::Server {
            name: "A".to_string(),
            url: "http://127.0.0.1:1".to_string(),
            command: "echo a".to_string(),
            timeout: 1,
        }],
        command: "echo done".to_string(),
    };
    let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));

    mark_server_stopped(&state, 0);

    assert_eq!(state.lock().unwrap().servers[0].status, ServerStatus::Stopped);
}
```

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test restart_resets_server_state`

Expected: FAIL because `reset_server_for_start` does not exist.

Run: `cargo test stop_marks_server_stopped`

Expected: FAIL because `mark_server_stopped` does not exist.

- [ ] **Step 3: Implement state helpers**

Add to `src/core/actor.rs`:

```rust
fn reset_server_for_start(state: &Arc<Mutex<AppState>>, idx: usize, reason: &str) {
    if let Ok(mut guard) = state.lock() {
        if let Some(server) = guard.servers.get_mut(idx) {
            server.status = ServerStatus::Waiting;
            server.attempts = crate::core::state::Attempts(0);
            if let Ok(mut log) = server.log.lock() {
                log.push(format!("--- {reason} ---"));
            }
        }
    }
}

fn mark_server_stopped(state: &Arc<Mutex<AppState>>, idx: usize) {
    if let Ok(mut guard) = state.lock() {
        if let Some(server) = guard.servers.get_mut(idx) {
            server.status = ServerStatus::Stopped;
            if let Ok(mut log) = server.log.lock() {
                log.push("--- Server stopped ---".to_string());
            }
        }
    }
}
```

- [ ] **Step 4: Implement command handling**

Update the `run_tui_engine` command match:

```rust
Some(EngineCommand::Restart(idx)) => {
    if let Some(process) = processes.get_mut(idx).and_then(Option::as_mut) {
        process.stop().await?;
    }
    let server = &config.servers[idx];
    let process = ServerProcess::spawn_captured(&server.name, &server.command)?;
    if let Some(slot) = processes.get_mut(idx) {
        *slot = Some(process);
    }
    reset_server_for_start(&state, idx, "Restarting server");
    final_started = false;
}
Some(EngineCommand::StopStart(idx)) => {
    let current = state.lock().unwrap().servers[idx].status;
    if current == ServerStatus::Stopped || current == ServerStatus::Failed {
        let server = &config.servers[idx];
        let process = ServerProcess::spawn_captured(&server.name, &server.command)?;
        if let Some(slot) = processes.get_mut(idx) {
            *slot = Some(process);
        }
        reset_server_for_start(&state, idx, "Starting server");
    } else {
        if let Some(process) = processes.get_mut(idx).and_then(Option::as_mut) {
            process.stop().await?;
        }
        if let Some(slot) = processes.get_mut(idx) {
            *slot = None;
        }
        mark_server_stopped(&state, idx);
    }
    final_started = false;
}
```

Keep bounds checks: if `idx >= config.servers.len()`, ignore the command.

- [ ] **Step 5: Run tests to verify GREEN**

Run: `cargo test restart_resets_server_state`

Expected: PASS.

Run: `cargo test stop_marks_server_stopped`

Expected: PASS.

- [ ] **Step 6: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 7: Commit**

```bash
git add src/core/actor.rs
git commit -m "feat: handle TUI server control commands"
```

---

## Task 10: Add Rerun Command Handling And Invalid Action Hints

**Files:**
- Modify: `src/core/actor.rs`
- Modify: `src/runner/tui/app.rs`

- [ ] **Step 1: Write failing TuiApp tests for final row and hints**

Append to `src/runner/tui/app.rs` tests:

```rust
#[test]
fn final_row_detection_uses_server_count() {
    let app = TuiApp::new(2);
    assert!(!app.is_final_row());

    let mut app = TuiApp::new(2);
    app.select_next();
    app.select_next();
    assert!(app.is_final_row());
}
```

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test final_row_detection_uses_server_count`

Expected: FAIL because `is_final_row` does not exist.

- [ ] **Step 3: Implement final row helper**

Add `server_count` field to `TuiApp`:

```rust
server_count: usize,
```

Set it in `TuiApp::new`:

```rust
server_count,
```

Add method:

```rust
pub fn is_final_row(&self) -> bool {
    self.selected == self.server_count
}
```

- [ ] **Step 4: Implement rerun handling in actor**

In `run_tui_engine`, handle `EngineCommand::RerunFinalCommand`:

```rust
Some(EngineCommand::RerunFinalCommand) => {
    let status = state.lock().unwrap().final_cmd.status;
    if all_servers_running(&state) && status != FinalCmdStatus::Running {
        run_final_command_for_tui(&config.command, &state).await?;
    }
}
```

- [ ] **Step 5: Run tests to verify GREEN**

Run: `cargo test final_row_detection_uses_server_count`

Expected: PASS.

- [ ] **Step 6: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 7: Commit**

```bash
git add src/core/actor.rs src/runner/tui/app.rs
git commit -m "feat: support final command rerun in TUI"
```

---

## Task 11: Wire TUI Runner, Terminal Guard, And Main Dispatch

**Files:**
- Modify: `src/runner/tui/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add a safe terminal guard test seam**

In `src/runner/tui/mod.rs`, add unit tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_guard_mode_tracks_mouse_capture_setting() {
        let mode = TerminalMode::new(true);
        assert!(mode.mouse_capture);
    }
}
```

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test terminal_guard_mode_tracks_mouse_capture_setting`

Expected: FAIL because `TerminalMode` does not exist.

- [ ] **Step 3: Implement terminal mode and runner skeleton**

Replace `src/runner/tui/mod.rs` with:

```rust
pub mod app;
pub mod input;
pub mod ui;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use std::io::{self, Stdout};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::Config;
use crate::core::actor::{EngineCommand, run_tui_engine};
use crate::core::state::AppState;
use crate::runner::tui::app::TuiApp;
use crate::runner::tui::input::{InputEvent, UiAction, map_event};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalMode {
    pub mouse_capture: bool,
}

impl TerminalMode {
    pub fn new(mouse_capture: bool) -> Self {
        Self { mouse_capture }
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        previous(info);
    }));
}

pub async fn run(config: Config, max_attempts: u8) -> anyhow::Result<()> {
    install_panic_hook();
    let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));
    let mut app = TuiApp::new(config.servers.len());
    let (tx, rx) = mpsc::channel(32);
    let actor_state = Arc::clone(&state);
    let actor = tokio::spawn(async move { run_tui_engine(config, max_attempts, actor_state, rx).await });

    run_terminal_loop(&mut app, Arc::clone(&state), tx.clone()).await?;

    let _ = tx.send(EngineCommand::Quit).await;
    let _ = actor.await;
    Ok(())
}

async fn run_terminal_loop(
    app: &mut TuiApp,
    state: Arc<Mutex<AppState>>,
    tx: mpsc::Sender<EngineCommand>,
) -> anyhow::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal: Terminal<CrosstermBackend<Stdout>> = Terminal::new(backend)?;

    loop {
        terminal.draw(|frame| ui::render(frame, app, &state))?;
        if app.should_quit() {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => handle_action(app, map_event(InputEvent::Key(key)), &state, &tx).await?,
                Event::Mouse(mouse) => handle_action(app, map_event(InputEvent::Mouse(mouse)), &state, &tx).await?,
                _ => {}
            }
        }
    }
    Ok(())
}

async fn handle_action(
    app: &mut TuiApp,
    action: UiAction,
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
            let can_rerun = state
                .lock()
                .map(|state| state.servers.iter().all(|server| server.status == crate::core::state::ServerStatus::Running))
                .unwrap_or(false);
            if can_rerun {
                tx.send(EngineCommand::RerunFinalCommand).await?;
            } else {
                app.set_footer_message("All servers must be running before re-run");
            }
        }
        UiAction::Quit => {
            tx.send(EngineCommand::Quit).await?;
            app.request_quit();
        }
        other => app.apply_action(other),
    }
    Ok(())
}
```

This uses blocking `crossterm::event::poll/read` inside the async loop with a short timeout. Do not use `EventStream`; this simpler loop is enough for Phase 2.

- [ ] **Step 4: Update `main.rs` dispatch**

Change `async_main` in `src/main.rs`:

```rust
async fn async_main(args: Args) -> anyhow::Result<()> {
    let config = config::get_config(&args.config)?;

    if args.tui {
        runner::tui::run(config, args.attempts).await
    } else {
        runner::plain::run(config, args.attempts).await
    }
}
```

- [ ] **Step 5: Run tests to verify GREEN**

Run: `cargo test terminal_guard_mode_tracks_mouse_capture_setting`

Expected: PASS.

- [ ] **Step 6: Verify CLI no longer shows inert TUI error**

Run: `cargo run -- --help`

Expected: help still includes `--tui`.

Do not run `cargo run -- --tui` in a non-interactive command as a pass/fail check; it opens the TUI and waits for input.

- [ ] **Step 7: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 8: Commit**

```bash
git add src/runner/tui/mod.rs src/main.rs
git commit -m "feat: wire TUI runner"
```

---

## Task 12: Improve Actor Quit While Final Command Runs

**Files:**
- Modify: `src/core/actor.rs`
- Modify: `src/core/command.rs`

Task 8's simple final-command lifecycle may block actor commands while the final command runs. This task makes final command execution a task so Quit remains responsive.

- [ ] **Step 1: Add final-command task state test seam**

Append to `src/core/actor.rs` tests:

```rust
#[test]
fn final_running_status_blocks_auto_rerun() {
    let config = crate::config::Config {
        servers: Vec::new(),
        command: "echo done".to_string(),
    };
    let state = Arc::new(Mutex::new(AppState::new(&config.servers, &config.command)));
    set_final_status(&state, FinalCmdStatus::Running);

    assert!(!final_command_is_idle(&state));
    set_final_status(&state, FinalCmdStatus::Succeeded(0));
    assert!(final_command_is_idle(&state));
}
```

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test final_running_status_blocks_auto_rerun`

Expected: FAIL because `final_command_is_idle` does not exist.

- [ ] **Step 3: Add helper**

Add to `src/core/actor.rs`:

```rust
fn final_command_is_idle(state: &Arc<Mutex<AppState>>) -> bool {
    state
        .lock()
        .map(|state| state.final_cmd.status != FinalCmdStatus::Running)
        .unwrap_or(false)
}
```

- [ ] **Step 4: Refactor final command to spawned task**

Inside `run_tui_engine`, replace direct calls to `run_final_command_for_tui(...).await?` with a task handle:

```rust
let mut final_task: Option<tokio::task::JoinHandle<anyhow::Result<()>>> = None;
```

When all servers are running:

```rust
if !final_started && all_servers_running(&state) && final_command_is_idle(&state) {
    final_started = true;
    let command = config.command.clone();
    let state_clone = Arc::clone(&state);
    final_task = Some(tokio::spawn(async move {
        run_final_command_for_tui(&command, &state_clone).await
    }));
}
```

Add a select branch:

```rust
result = async {
    match final_task.as_mut() {
        Some(task) => Some(task.await),
        None => None,
    }
}, if final_task.is_some() => {
    final_task = None;
    if let Some(joined) = result {
        joined??;
    }
}
```

On `Quit`, abort any running task before stopping servers:

```rust
if let Some(task) = final_task.take() {
    task.abort();
}
stop_all(&mut processes).await?;
return Ok(());
```

On `RerunFinalCommand`, spawn the same task only when `all_servers_running(&state)` and `final_command_is_idle(&state)` are true.

- [ ] **Step 5: Run tests to verify GREEN**

Run: `cargo test final_running_status_blocks_auto_rerun`

Expected: PASS.

- [ ] **Step 6: Verify full suite and lint**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS. If formatting fails, run `cargo fmt` and re-run.

- [ ] **Step 7: Commit**

```bash
git add src/core/actor.rs src/core/command.rs
git commit -m "feat: keep TUI actor responsive during final command"
```

---

## Task 13: Add TUI Manual Verification And Final Polish

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Run full automated verification**

Run: `cargo test`

Expected: PASS.

Run: `cargo clippy -- -D warnings`

Expected: PASS.

Run: `cargo fmt -- --check`

Expected: PASS.

Run: `cargo run -- --help`

Expected: help includes `--tui`.

- [ ] **Step 2: Manual interactive TUI check**

Run: `cargo run -- --tui`

Expected:

- TUI opens in alternate screen.
- Server rows appear in the sidebar.
- Logs render in the right pane.
- Final command auto-runs after servers are ready.
- TUI remains open after final command completion.
- `↑` / `↓` or `j` / `k` changes selection.
- `PgUp` / `PgDn` scrolls logs.
- Mouse wheel scrolls logs for the currently selected pane.
- `End` returns to tail-following.
- `r` restarts a selected server.
- `s` stops and starts a selected server.
- `e` re-runs the final command when all servers are running.
- `q` exits and restores the terminal.

- [ ] **Step 3: Verify cleanup after manual run**

Run: `ss -tlnp sport = :3000`

Expected: no listener from server-runner's test server remains.

- [ ] **Step 4: Update README**

In `README.md`, add `--tui` to the options list after `--attempts`:

```markdown
- `--tui`: run the interactive control panel with live status, logs, and server controls.
```

- [ ] **Step 5: Final diff review**

Run: `git status --short`

Expected: only intended files modified.

Run: `git diff --stat main..HEAD`

Expected: changes align with this Phase 2 plan.

- [ ] **Step 6: Commit final polish**

```bash
git add README.md
git commit -m "docs: document TUI mode"
```

---

## Final Verification

- [ ] Run: `cargo test` — expected PASS.
- [ ] Run: `cargo clippy -- -D warnings` — expected PASS.
- [ ] Run: `cargo fmt -- --check` — expected PASS.
- [ ] Run: `cargo run -- --help` — expected `--tui` in help.
- [ ] Manual: `cargo run -- --tui` — expected full interactive behavior listed in Task 13.
- [ ] Manual: after quitting TUI, `ss -tlnp sport = :3000` shows no leftover server process.

## Spec Coverage

- Full opt-in `--tui` path: Tasks 1, 11, 13.
- Engine actor plus shared `AppState`: Tasks 2, 7, 8, 9, 10, 12.
- Sidebar + detail/log pane rendering: Task 5.
- Keyboard controls: Tasks 3, 4, 10, 11.
- Mouse-wheel log scrolling: Task 4 and Task 13.
- Terminal safety: Task 11 and Task 13.
- Plain-mode preservation: every task runs `cargo test`; Task 11 preserves `runner::plain` dispatch for non-TUI mode.
