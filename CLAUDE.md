# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Server Runner is a Rust CLI tool that starts multiple web servers, waits for them to be ready (HTTP 200 responses), then executes a command when all servers are running. It's designed for automated testing workflows where you need to spin up multiple services before running tests.

## Development Commands

### Building
```bash
cargo build                    # Debug build
cargo build --release          # Release build
```

### Testing
```bash
cargo test                     # Run all tests
cargo test -- --nocapture      # Run tests with stdout output
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

## Architecture

### Core Components

**Main Flow** (`src/main.rs:75-142`):
1. Parse CLI arguments and load YAML configuration
2. Start all server processes concurrently 
3. Poll each server URL until HTTP 200 or max attempts reached
4. Execute the final command when all servers are ready
5. Clean up all processes on completion or failure

**Key Structures**:
- `Config` - Deserializes YAML with servers list and final command
- `Server` - Individual server configuration (name, URL, command)
- `ServerProcess` - Running process wrapper with name and Child process
- `ServerStatus` - Enum tracking Waiting/Running states
- `Attempts` - Newtype for attempt counting with custom operators

**Process Management**:
- Uses `std::process::Command` to spawn server processes
- Implements Ctrl+C handler for graceful shutdown
- Cross-platform process creation (Windows-specific flags)
- Automatic cleanup of all child processes

**HTTP Health Checks**:
- Uses `reqwest::blocking` for synchronous HTTP requests
- Distinguishes between connection errors (retry) and other failures (abort)
- Configurable retry attempts with 1-second intervals

### Configuration Format

YAML configuration with two main sections:
```yaml
servers:                    # List of servers to start
  - name: "Server Name"
    url: "http://localhost:8080"
    command: "node server.js"
command: "npm test"         # Command to run when all servers ready
```

Example configs in repository:
- `servers.yaml` - Basic example with simple-http-server
- `max_attempts.yaml` - Test config for connection failure scenarios

### Testing Strategy

Uses `assert_cmd` for CLI integration tests (`tests/cli.rs`):
- Success case with default config
- Missing config file error handling  
- Max attempts exceeded scenarios
- Custom attempt limit configuration

Tests require `simple-http-server` to be installed for the working config scenario.