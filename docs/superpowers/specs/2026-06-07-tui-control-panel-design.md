# TUI Control Panel — Design

Date: 2026-06-07
Status: Approved (pending implementation plan)

## Summary

Add an opt-in interactive TUI ("control panel") to server-runner, activated with
a new `--tui` flag. The TUI shows live server readiness, captured per-server and
final-command output, and lets the user restart/stop individual servers, re-run
the final command, scroll logs, and quit gracefully.

The change introduces a unified **async (tokio)** core engine shared by both the
existing plain-log mode and the new TUI mode, and splits the current single-file
`src/main.rs` into focused modules. Plain mode (no `--tui`) must remain
observably identical, with all existing integration tests passing unchanged.

## Decisions (from brainstorming)

- **Scope:** Full interactive control panel (status + logs + interactivity).
- **Activation:** Opt-in via `--tui`. Plain logs remain the default everywhere.
- **Library:** `ratatui` + `crossterm`.
- **Concurrency:** Async with `tokio`.
- **Migration:** Unify both modes on a single async core (convert readiness
  checks to async `reqwest`; no separate blocking implementation).
- **Interactions:** quit, restart server, stop/start server, re-run final
  command, scroll a server's log pane, switch focus between entries.
- **Post-command lifecycle:** TUI stays open after the final command finishes,
  shows its result, and waits for the user.
- **Failure handling (TUI):** mark the server FAILED and keep the panel alive;
  failures are recoverable via restart. Plain mode keeps today's abort semantics.
- **Layout:** Layout A — server sidebar + log detail pane.

## Architecture & Module Layout

```
src/
  main.rs        # arg parsing, runtime bootstrap, dispatch to runner
  cli.rs         # Args (adds --tui flag)
  config.rs      # Config/Server structs, get_config, validation
  core/
    mod.rs       # shared async engine
    server.rs    # ServerProcess, spawn, kill/restart, output capture
    health.rs    # async readiness check (async reqwest)
    state.rs     # ServerStatus, Attempts, ServerName, shared AppState
    command.rs   # final command spawn/run
  runner/
    plain.rs     # non-TUI mode: async poll loop -> logs (current behavior)
    tui/
      mod.rs     # TUI event loop, runs the engine + renders
      app.rs     # TUI-specific state (selection, scroll, focus)
      ui.rs      # ratatui rendering (Layout A)
      input.rs   # key handling -> actions
```

**Shared async engine:** the `core` module owns the server processes, runs
readiness polling, captures output, and exposes shared state behind
`Arc<Mutex<AppState>>` plus an mpsc command channel. Both runners drive the same
engine:

- `runner::plain` — awaits engine events, emits `log` lines, aborts on failure,
  and exits with the final command's status (preserves today's semantics and
  exit code).
- `runner::tui` — drives the engine, renders `AppState` each frame, and
  translates keypresses into engine commands.

**Behavior guarantee:** plain mode (no `--tui`) must remain observably identical;
all existing `tests/cli.rs` integration tests stay green unchanged.

## Dependencies

- Add: `tokio` (features: rt-multi-thread, process, sync, time, macros),
  `ratatui`, `crossterm`.
- `reqwest`: drop `blocking`, use the default async client.
- Keep: `log` + `simplelog` for plain mode; `command-group` for process-group
  cleanup; `shlex`, `clap`, `anyhow`, `config`, `serde`, `ctrlc`.

## Async Core Engine

### State model (`core/state.rs`)

```rust
enum ServerStatus { Waiting, Running, Failed, Stopped }   // adds Failed, Stopped

struct ServerRuntime {
    name, url, command, timeout,
    status: ServerStatus,
    attempts: Attempts,          // existing newtype, preserved
    log: RingBuffer<String>,     // bounded, ~5000 lines (constant)
}

enum FinalCmdStatus { Idle, Running, Succeeded(i32), Failed(i32) }

struct AppState {
    servers: Vec<ServerRuntime>,
    final_cmd: FinalCmd,         // { status, log: RingBuffer<String> }
}
```

`AppState` lives behind `Arc<Mutex<_>>` (tokio mutex). `Attempts`, `ServerName`,
validation, and `Config` move out of `main.rs` unchanged in behavior.

### Engine API (`core/mod.rs`)

- `Engine::start(config) -> Engine` — spawns all servers, begins polling.
- Accepts commands over an mpsc channel: `Restart(idx)`, `StopStart(idx)`,
  `RerunCommand`, `Quit`.
- Emits events / mutates `AppState` that runners read: status changes, new log
  lines, final-command completion.

### Polling

One async task per server (or a single ticker iterating all). Each tick:

- `health::check(url, timeout)` using async `reqwest`. Same logic preserved:
  connect-error -> still `Waiting`; non-success status -> `Waiting`; success ->
  `Running`; `is_connect()` semantics preserved.
- Increment `Attempts`; on reaching `max_attempts` -> `Failed` (TUI) / abort
  (plain, unchanged).
- `tokio::time::sleep(1s)` between rounds, replacing `thread::sleep`.

### Final command

Auto-runs once when all servers first reach `Running`; afterward only via
`RerunCommand`. Spawned through the existing `shlex` + `build_command` path
(kept, minus blocking specifics).

### Notable changes from today

- `ServerStatus` gains `Failed` / `Stopped` variants.
- Per-server output is captured (piped) rather than inherited. In plain mode,
  captured lines are forwarded to log output to preserve visible behavior.

## TUI Subsystem

### Output capture (`core/server.rs`)

Servers spawn with piped stdout/stderr (`tokio::process` + `command-group` for
process-group cleanup). One reader task per stream reads lines and pushes into
that server's `RingBuffer`, tagged so both streams interleave in order. Same
mechanism for the final command's output.

### TUI app state (`runner/tui/app.rs`)

```rust
struct TuiApp {
    selected: usize,        // index into [servers..., final_cmd]
    scroll: ScrollState,    // per-selection scroll; follows tail unless scrolled up
    should_quit: bool,
}
```

### Event loop (`runner/tui/mod.rs`)

`tokio::select!` over three sources:

- crossterm input events (via `EventStream`),
- a render tick (~16–33 ms, redraw from current `AppState`),
- engine state updates.

Enter raw mode + alternate screen on start; restore terminal on exit via a guard
so a panic or error never leaves the terminal corrupted.

### Rendering — Layout A (`runner/tui/ui.rs`)

```
┌ server-runner ────────────────────────────────────────────────┐
│ Servers              │ Logs: API server                        │
│ ▶ ● API server  RUN  │ [api] listening on :8080                │
│   ◐ Worker     WAIT 3│ [api] connected to db                   │
│   ✗ Cache      FAIL  │ [api] ready                              │
│   ⚙ npm test   IDLE  │                                         │
├──────────────────────┴─────────────────────────────────────────┤
│ ↑/↓ select  r restart  s stop/start  e re-run  q quit          │
└────────────────────────────────────────────────────────────────┘
```

- Left list: every server + the final command as the last entry; `▶` marks
  selection.
- Status glyph/color: `●` green RUNNING, `◐` yellow WAITING (with attempt
  count), `✗` red FAILED, `○` gray STOPPED, `⚙` final-command
  IDLE/RUNNING/result.
- Right pane: focused entry's log buffer, wrapped, auto-tailing; honors scroll
  offset when the user scrolls up.
- Bottom: context-sensitive keybinding hints.

### Input mapping (`runner/tui/input.rs`)

Maps to engine commands or local view changes:

- `↑/↓` or `Tab` move `selected`; `PgUp/PgDn` (or `j/k`) scroll the log pane.
- `r` -> `Restart(selected)`; `s` -> `StopStart(selected)`; `e` ->
  `RerunCommand`; `q` / `Ctrl+C` -> `Quit`.
- Actions targeting the final-command entry: `e` re-runs; `r` / `s` are no-ops.

## Lifecycle, Failure, Exit

### Auto-run + ready transition

The engine auto-runs the final command once when all servers first reach
`RUNNING`. In the TUI, `final_cmd` flips `IDLE -> RUNNING -> Succeeded/Failed`;
output streams into its pane. Re-run (`e`) is allowed only when all servers are
`RUNNING`; otherwise it is a no-op with a brief hint.

### Restart / stop-start (TUI only)

- `Restart(idx)`: kill the process group, respawn, reset `status -> Waiting`,
  `attempts -> 0`, mark a separator in its log buffer, resume polling.
- `StopStart(idx)`: if running/waiting -> kill group, `status -> Stopped`,
  polling paused; if stopped -> respawn, `status -> Waiting`, polling resumes.
- While any server is not `Running`, the final command stays blocked (won't
  auto-run).

### Failure (TUI)

Exceeding `max_attempts` sets `status -> Failed`; other servers and the panel
keep running; logs remain inspectable; `Restart` recovers it. Plain mode keeps
today's abort-and-exit semantics unchanged.

### Quit / teardown

`q` / `Ctrl+C` triggers `Quit`: stop all server process groups and any running
final command (reusing existing group-kill logic), restore the terminal via the
guard, then exit. Exit code: `0` on clean interactive quit. Plain mode's exit
code is unchanged (final command status / error).

### Terminal safety

A `TerminalGuard` (Drop) restores cooked mode and leaves the alternate screen
even on panic or error, so the user's shell is never left broken. A panic hook
also restores the terminal before printing the panic.

## Testing Strategy

- Existing `tests/cli.rs` integration tests run unchanged -> proves plain-mode
  parity after the async migration.
- Unit tests on pure logic: status transitions, `Attempts` saturation,
  ring-buffer bounding/tailing, input->command mapping, selection/scroll math.
- `health.rs`: async readiness check tests (success / redirect / connect-error
  -> Waiting; non-success -> Waiting) mirroring current behavior.
- TUI rendering: render `AppState` to a ratatui `TestBackend` buffer and assert
  on produced cells (status glyphs, selection marker, log contents) — no real
  terminal needed.
- Engine command tests: feed `Restart` / `StopStart` / `Rerun` / `Quit` and
  assert resulting `AppState`, using a stub/echo server command.

## Out of Scope (YAGNI)

- TTY auto-detection / making the TUI default (explicitly opt-in only).
- Configurable themes, mouse support, log persistence to disk.
- Configurable log buffer size (fixed constant for now).
- `--exit-on-complete`-style one-shot TUI behavior.
