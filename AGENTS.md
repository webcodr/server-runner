# AGENTS.md

This file provides guidance to agentic coding agents working in the Server Runner codebase.

## Project Overview

Server Runner is a Rust CLI tool that starts multiple web servers, waits for them to be ready (HTTP 200 responses), then executes a command when all servers are running. It's designed for automated testing workflows.

## Build, Lint, and Test Commands

### Building
```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo check                    # Fast compilation check without code generation
```

### Linting
```bash
cargo clippy                   # Run Clippy linter
cargo clippy -- -D warnings    # Treat warnings as errors
cargo fmt -- --check           # Check formatting without modifying files
cargo fmt                      # Format all Rust code
```

### Testing
```bash
cargo test                     # Run all tests
cargo test -- --nocapture      # Run tests with stdout output
cargo test <test_name>         # Run a single test by name
cargo test --test cli          # Run all tests in tests/cli.rs
cargo test runs                # Run single test function: tests/cli.rs::runs
```

Examples of running specific tests:
```bash
cargo test fails_on_missing_config_file          # Single integration test
cargo test fails_on_timeout                      # Matches multiple tests with "timeout"
cargo test -- --test-threads=1                   # Run tests serially
```

### Running
```bash
cargo run                      # Run with default config (servers.yaml)
cargo run -- -c config.yaml    # Run with custom config
cargo run -- -v                # Run with verbose output
cargo run -- -a 5              # Run with custom max attempts (default: 10)
```

### Installation
```bash
cargo install --path .         # Install locally from source
```

## Code Style Guidelines

### Imports
- Group imports in this order: external crates, std library, local modules
- Use explicit imports rather than glob imports (avoid `use foo::*;`)
- Example from `src/main.rs:1-12`:
  ```rust
  use anyhow::{bail, Context};
  use clap::Parser;
  use log::info;
  use std::collections::HashMap;
  use std::ops::AddAssign;
  #[cfg(windows)]
  use std::os::windows::process::CommandExt;
  use std::process::{Child, Command};
  ```

### Formatting
- Use `rustfmt` with default settings (no custom config file)
- 4-space indentation
- Line length: Follow rustfmt defaults (~100 chars)
- Trailing commas in multi-line structures
- Use `cargo fmt` before committing

### Types and Type Annotations
- Use explicit types for struct fields: `name: String`, `timeout: u64`
- Leverage type inference in local variables when clear from context
- Use newtype pattern for domain concepts: `struct Attempts(u8)`, `struct ServerName(String)`
- Implement trait bounds explicitly: `impl AddAssign<u8> for Attempts`
- Use `anyhow::Result<T>` for error handling in functions that can fail
- Prefer owned types (`String`, `Vec<T>`) in structs; use references (`&str`, `&[T]`) in function parameters

### Naming Conventions
- Types: PascalCase (`ServerProcess`, `ServerStatus`)
- Functions: snake_case (`run_command`, `check_server`)
- Variables: snake_case (`server_processes`, `max_attempts`)
- Constants: SCREAMING_SNAKE_CASE (if needed)
- Enums: PascalCase for type and variants (`ServerStatus::Waiting`)
- CLI args: kebab-case in help text, snake_case in struct fields

### Error Handling
- Use `anyhow` for error handling and context propagation
- Use `?` operator for error propagation: `let config = get_config(&args.config)?;`
- Add context to errors: `.context(format!("Could not find config file {}", filename))?`
- Use `bail!` macro for early returns with errors: `bail!("Configuration must include at least one server")`
- Pattern match on specific error types when needed (e.g., `error.is_connect()` in `src/main.rs:280`)
- Return `anyhow::Result<T>` from fallible functions
- Exit with descriptive error messages in `main()` via `exit_with_error()`

### Serde and Deserialization
- Use `#[derive(serde::Deserialize)]` for config structs
- Provide default values with `#[serde(default = "function_name")]`
- Example: `#[serde(default = "default_timeout")]` on `timeout` field
- Validate deserialized config immediately after parsing (see `get_config()`)

### Logging
- Use `log` crate with `simplelog` implementation
- Use appropriate levels: `info!()` for progress, `eprintln!()` for errors
- Include context in log messages: `info!("Starting server {}", s.name)`
- Respect verbose flag for log level control

### Cross-Platform Code
- Use `#[cfg(windows)]` for Windows-specific code
- Example: `cmd.creation_flags(0x08000000);` for Windows process creation
- Test on both Unix and Windows when making process-related changes

### Concurrency and Shared State
- Use `Arc<Mutex<T>>` for shared mutable state across threads
- Clone `Arc` before moving into closures: `let clone = Arc::clone(&original);`
- Always call `.lock()` on Mutex before accessing inner data
- Handle `LockResult` properly (see `stop_servers()` function)

### Custom Types and Operators
- Implement common traits for custom types: `Display`, `PartialEq`, `AddAssign`, etc.
- Use operator overloading sparingly and only when semantically clear
- Example: `Attempts` implements `AddAssign<u8>` and `PartialEq<u8>` for ergonomic use

### Testing
- Use `assert_cmd` for CLI integration tests
- Use `predicates` for assertion matching
- Test both success and failure cases
- Test edge cases: empty configs, timeouts, invalid input
- Place test fixtures in `tests/` directory (YAML configs)
- Test naming: descriptive snake_case describing what fails/succeeds

## Architecture Notes

### Main Flow (`src/main.rs:81-151`)
1. Parse CLI args and load YAML configuration
2. Start all server processes concurrently
3. Poll each server URL until HTTP 200 or max attempts reached
4. Execute final command when all servers are ready
5. Clean up all processes on completion or failure

### Key Structures
- `Config`: YAML config with `servers` list and `command` to run
- `Server`: Individual server config (name, URL, command, timeout)
- `ServerProcess`: Running process wrapper
- `ServerStatus`: Enum for Waiting/Running states
- `Attempts`: Newtype for attempt counting

### Process Management
- Use `std::process::Command` to spawn processes
- Implement Ctrl+C handler for graceful shutdown via `ctrlc` crate
- Kill all child processes on error or completion
- Use `shlex` for safe command parsing

## Configuration

### YAML Format
```yaml
servers:                    # List of servers to start
  - name: "Server Name"
    url: "http://localhost:8080"
    command: "node server.js"
    timeout: 5              # Optional, defaults to 5 seconds
command: "npm test"         # Command to run when all servers ready
```

## Common Tasks

### Adding a New CLI Argument
1. Add field to `Args` struct with `#[arg(...)]` attribute
2. Pass argument through to relevant function
3. Add test case in `tests/cli.rs`

### Adding a New Config Field
1. Add field to `Server` or `Config` struct with `#[serde(...)]`
2. Provide default value function if optional
3. Update validation logic if needed
4. Add test YAML fixture in `tests/`

### Adding a New Test
1. Create test function in `tests/cli.rs` with `#[test]`
2. Use `Command::cargo_bin("server-runner")` to get binary
3. Add args with `.arg()` method
4. Assert with `.assert().success()` or `.failure()`
5. Check stderr/stdout with `predicate::str::contains()`

## Dependencies

Key dependencies and their purposes:
- `anyhow`: Error handling and context
- `clap`: CLI argument parsing (derive API)
- `config`: YAML configuration file parsing
- `ctrlc`: Signal handling for graceful shutdown
- `log` + `simplelog`: Logging infrastructure
- `reqwest` (blocking): HTTP client for health checks
- `serde`: Serialization/deserialization
- `shlex`: Shell-like command parsing

## Version Information

- Edition: 2024
- Current version: 1.6.0
- MSRV: Not explicitly specified (uses 2024 edition features)
