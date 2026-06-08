# TUI Control Panel Phase 2 — Design

Date: 2026-06-08
Status: Approved for planning

## Summary

Phase 2 implements the full opt-in TUI control panel behind `--tui`, using
`ratatui` and `crossterm`. The TUI shows live server status, per-server logs,
final-command logs, and interactive controls for quit, restart, stop/start,
re-run final command, keyboard log scrolling, and mouse-wheel log scrolling.

Plain mode remains on the verified Phase 1 async runner and must stay
observably unchanged.

## Decisions

- Use an engine actor plus shared `AppState` for the TUI path.
- Leave `runner::plain` on its current verified code path.
- Implement the full approved control panel in this phase.
- Keep the approved sidebar + detail/log pane layout.
- Add mouse support only for log scrolling. Mouse clicks are ignored for Phase 2.
- Keep `--tui` opt-in; plain mode remains the default.

## Architecture

Phase 2 adds a TUI-specific engine actor and TUI runner modules:

```text
src/
  core/
    actor.rs       # TUI engine actor: owns processes, polling, commands, AppState updates
  runner/
    tui/
      mod.rs       # terminal lifecycle + async event/render loop
      app.rs       # UI-local selection, scroll, focus, transient messages
      input.rs     # key/mouse event -> action/engine command mapping
      ui.rs        # ratatui rendering with TestBackend tests
```

`main.rs` dispatches by mode:

```rust
if args.tui {
    runner::tui::run(config, args.attempts).await
} else {
    runner::plain::run(config, args.attempts).await
}
```

The TUI runner owns terminal setup/teardown and rendering. The actor owns
server processes, readiness polling, final command execution, and cleanup. The
UI reads `Arc<Mutex<AppState>>` and sends commands over a channel; it never
touches child processes directly.

## Engine Actor

The actor API:

```rust
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
    commands: mpsc::Receiver<EngineCommand>,
) -> anyhow::Result<()>;
```

Actor behavior:

- Initialize `AppState.servers` from config with `Waiting`, `Attempts(0)`, and
  per-server log buffers.
- Spawn all configured servers at startup and store process handles internally.
- Poll waiting servers once per second using the existing async readiness check.
- Increment attempts for waiting servers.
- On readiness success, set the server to `Running`.
- In TUI mode, max attempts marks the server `Failed` and keeps the panel alive.
- Auto-run the final command once when all servers first reach `Running`.
- Allow `RerunFinalCommand` only when all servers are `Running` and the final
  command is not currently running.
- `Restart(i)` stops and respawns server `i`, resets attempts to `0`, sets status
  to `Waiting`, and appends a separator/status line to that server log.
- `StopStart(i)` stops active servers into `Stopped`; for stopped or failed
  servers, it respawns and sets status to `Waiting`.
- `Quit` stops all server process groups and any running final command, then
  exits the actor.

The actor uses `tokio::select!` over a one-second polling interval, engine
commands, and final-command completion.

## TUI State And Rendering

`runner::tui::app` owns UI-local state:

```rust
pub struct TuiApp {
    selected: usize,
    scroll: Vec<usize>,
    follow_tail: Vec<bool>,
    footer_message: Option<String>,
    should_quit: bool,
}
```

Selection indexes `[servers..., final_cmd]`, with the final command as the last
row. Each selectable row has independent scroll state. Logs auto-tail until the
user scrolls up; `End` returns the focused pane to tail-following mode.

The layout uses the approved sidebar + detail pane:

```text
┌ server-runner ────────────────────────────────────────────────┐
│ Servers              │ Logs: API server                        │
│ ▶ ● API server  RUN  │ [api] listening on :8080                │
│   ◐ Worker     WAIT 3│ [api] connected to db                   │
│   ✗ Cache      FAIL  │ [api] ready                              │
│   ⚙ npm test   OK 0  │                                         │
├──────────────────────┴─────────────────────────────────────────┤
│ ↑/↓ select  PgUp/PgDn scroll  r restart  s stop/start  e rerun  q quit │
└────────────────────────────────────────────────────────────────┘
```

Status display:

- Server rows: `WAIT <attempts>`, `RUN`, `FAIL`, `STOP`.
- Final command row: `IDLE`, `RUN`, `OK <code>`, `ERR <code>`.
- Invalid actions update the footer with a short hint instead of exiting.

## Input And Mouse Support

Keyboard mapping:

- `q` or `Ctrl+C`: quit, stop all processes, restore terminal, exit `0`.
- `↑` / `↓` or `k` / `j`: move selection.
- `Tab`: cycle selection.
- `PgUp` / `PgDn`: scroll the selected log pane.
- `End`: return selected log pane to tail-following mode.
- `r`: restart selected server; no-op with footer hint on final-command row.
- `s`: stop/start selected server; no-op with footer hint on final-command row.
- `e`: re-run final command if all servers are running and the command is idle or
  done; otherwise show a footer hint.

Mouse mapping:

- Enable crossterm mouse capture for TUI mode.
- Mouse wheel up/down scrolls the currently selected log pane.
- Mouse wheel uses a smaller increment than `PgUp` / `PgDn`.
- Mouse clicks are ignored in Phase 2; selection remains keyboard-only.

## Lifecycle And Terminal Safety

`runner::tui::mod` provides a `TerminalGuard`:

- On entry: enable raw mode, enter alternate screen, enable mouse capture.
- On drop: disable mouse capture, leave alternate screen, disable raw mode.
- A panic hook restores the terminal before panic output is printed.

Quit behavior:

- `q` and `Ctrl+C` send `EngineCommand::Quit`.
- The runner waits for actor cleanup.
- The terminal is restored on every exit path.
- Clean interactive quit exits with code `0`.

Failure behavior:

- Server readiness exhaustion marks that server `Failed` and keeps the panel
  alive.
- Restart/stop/start errors append a log/status line and mark the server
  `Failed`; they do not corrupt the terminal.
- Final command failure sets `FinalCmdStatus::Failed(code)` and keeps the panel
  alive.

## Dependencies

Add:

- `ratatui`
- `crossterm` with event support needed for keyboard and mouse input

Keep existing Phase 1 async dependencies. Plain-mode logging remains on `log` and
`simplelog`.

## Testing Strategy

- Existing `tests/cli.rs` continues to prove plain-mode behavior.
- Unit tests for input mapping, including keyboard and mouse-wheel events.
- Unit tests for selection, scroll math, and auto-tail behavior.
- `ratatui::backend::TestBackend` rendering tests for sidebar statuses,
  selection marker, selected log pane, final command row, and footer hints.
- Engine actor tests for max-attempt failure, restart, stop/start, rerun, and
  quit cleanup.
- TUI terminal guard tests should avoid requiring a real interactive terminal;
  test pure logic directly and keep terminal side effects isolated.

## Out Of Scope

- Making TUI the default.
- Mouse clicks or mouse selection.
- Configurable themes.
- Persisting logs to disk.
- Configurable log buffer size.
- A one-shot `--exit-on-complete` TUI mode.

## Success Criteria

- `cargo test` passes.
- `cargo clippy -- -D warnings` passes.
- `cargo fmt -- --check` passes.
- `cargo run -- --tui` opens the TUI, starts servers, shows live logs/status, and
  stays open after final command completion.
- `q` and `Ctrl+C` restore the terminal and leave no orphan server processes.
- Keyboard controls work for selection, scrolling, restart, stop/start, rerun,
  and quit.
- Mouse wheel scrolls the currently selected log pane.
- Plain mode remains unchanged and all existing CLI integration tests pass.
