# Server Runner v2.0

![GitHub](https://img.shields.io/badge/github-webcodr/server--runner-8da0cb?style=for-the-badge&logo=github&labelColor=555555)
![Crates.io Version](https://img.shields.io/crates/v/server-runner?style=for-the-badge&logo=rust&color=fc8d62)
![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/webcodr/server-runner/build.yml?style=for-the-badge)

Server Runner is a powerful Rust CLI tool that orchestrates multiple web servers, monitors their health, and executes commands when all servers are ready. Perfect for automated testing workflows, development environments, and CI/CD pipelines.

## What's New in v2.0

Version 2.0 is a complete rewrite with many breaking changes and powerful new features:

- **Modular Architecture**: Refactored codebase with clean separation of concerns
- **Advanced Health Checks**: Custom HTTP methods, multiple success status codes, custom headers
- **Environment Variables**: Full support for env vars in configuration with `${VAR}` syntax
- **Lifecycle Hooks**: Run commands before/after server start and stop
- **Dependency Management**: Start servers in order based on dependencies and priorities
- **Flexible Output**: Control server output (capture, file, null, or inherit)
- **Better Error Handling**: Distinguish between retryable and fatal errors
- **Enhanced CLI**: More control with fail-fast, custom polling intervals, and startup timeouts

## Installation

### Via Cargo

```sh
cargo install server-runner
```

## Usage

```sh
server-runner [OPTIONS]
```

### CLI Options

- `-c, --config <FILE>` - Path to configuration file (default: `servers.yaml`)
- `-v, --verbose` - Enable verbose logging
- `-a, --attempts <NUMBER>` - Maximum connection attempts per server (default: 10)
- `-p, --poll-interval <SECONDS>` - Seconds between health check polls (default: 1)
- `-t, --startup-timeout <SECONDS>` - Maximum total seconds before giving up
- `-f, --fail-fast` - Stop immediately on first server failure
- `--continue-on-error` - Keep running even if some servers fail
- `-h, --help` - Print help information
- `-V, --version` - Print version information

### Example

```sh
server-runner -c config.yaml -v -a 15 -p 2 --fail-fast
```

## Configuration File

The configuration file uses YAML format with extensive customization options.

### Basic Example

```yaml
servers:
  - name: "Web Server"
    url: "http://localhost:8080"
    command: "node webserver.js"

  - name: "API Server"
    url: "http://localhost:3000/health"
    command: "python api_server.py"

command: "npm test"
```

### Advanced Example

```yaml
# Global environment variables
env:
  NODE_ENV: "production"
  LOG_LEVEL: "info"

servers:
  - name: "Database"
    url: "http://localhost:5432/health"
    command: "postgres -D data"
    priority: 1                    # Start first
    timeout: 10                    # HTTP timeout in seconds
    startup_delay: 2               # Wait before starting
    retry_interval: 2              # Seconds between health checks
    
    health_check:
      method: GET                  # HTTP method
      expected_status: [200, 204]  # Accept multiple status codes
      headers:
        Authorization: "Bearer ${DB_TOKEN}"
    
    env:
      POSTGRES_USER: "admin"
      POSTGRES_DB: "myapp"
    
    hooks:
      before_start: "npm run db:migrate"
      after_ready: "npm run db:seed"
      before_stop: "npm run db:backup"
    
    output:
      mode: "file"                 # capture, file, null, or inherit
      stdout: "logs/db.out"
      stderr: "logs/db.err"
      prefix: "[DB]"

  - name: "API Server"
    url: "http://localhost:3000/health"
    command: "node api.js"
    priority: 2                    # Start after priority 1
    depends_on: ["Database"]       # Wait for Database to be ready
    
    health_check:
      method: POST
      expected_status: [200]
      headers:
        Content-Type: "application/json"
    
    output:
      mode: "capture"
      prefix: "[API]"

command: "npm test"
```

### Configuration Fields

#### Global Fields

- `env` (optional): Environment variables for all servers and the final command
- `servers` (required): List of servers to start and monitor
- `command` (required): Command to execute when all servers are ready

#### Server Fields

**Required:**
- `name`: Display name for the server
- `url`: HTTP endpoint to check for availability
- `command`: Shell command to start the server

**Optional:**
- `timeout` (default: 5): HTTP request timeout in seconds
- `priority` (default: 0): Servers with lower priority start first
- `retry_interval` (default: 1): Seconds between health check attempts
- `startup_delay` (default: 0): Seconds to wait before starting server
- `depends_on` (default: []): List of server names this server depends on
- `env` (default: {}): Environment variables specific to this server

**Health Check Options:**
- `health_check.method` (default: "GET"): HTTP method (GET, POST, HEAD, PUT, PATCH, DELETE)
- `health_check.expected_status` (default: [200]): List of acceptable HTTP status codes
- `health_check.headers` (default: {}): Custom headers for health check requests

**Lifecycle Hooks:**
- `hooks.before_start`: Command to run before starting the server
- `hooks.after_ready`: Command to run after server passes health check
- `hooks.before_stop`: Command to run before stopping the server

**Output Configuration:**
- `output.mode` (default: "inherit"): How to handle server output
  - `inherit`: Pass through to terminal (default)
  - `capture`: Capture and optionally prefix output
  - `null`: Discard output
  - `file`: Write to files
- `output.stdout`: File path for stdout (when mode=file)
- `output.stderr`: File path for stderr (when mode=file)
- `output.prefix`: Prefix for output lines (when mode=capture)

### Environment Variable Expansion

Use `${VAR_NAME}` syntax in your configuration to reference environment variables:

```yaml
servers:
  - name: "API"
    url: "http://localhost:${API_PORT}/health"
    command: "node server.js"
    health_check:
      headers:
        Authorization: "Bearer ${API_TOKEN}"
    env:
      DATABASE_URL: "${DB_URL}"
```

## How It Works

1. **Load Configuration**: Parse YAML and expand environment variables
2. **Validate**: Check for circular dependencies and invalid settings
3. **Start by Priority**: Group servers by priority and start lower priorities first
4. **Check Dependencies**: Ensure dependent servers are ready before starting
5. **Run Hooks**: Execute `before_start` hooks
6. **Start Servers**: Launch server processes with configured output handling
7. **Health Checks**: Poll URLs until they return expected status codes
8. **Run Hooks**: Execute `after_ready` hooks when servers are healthy
9. **Execute Command**: Run the final command when all servers are ready
10. **Cleanup**: Stop all servers and run `before_stop` hooks

If any server fails after max attempts, Server Runner stops all servers and exits with an error (unless `--continue-on-error` is used).

## Error Handling

Server Runner distinguishes between different types of errors:

- **Connection Errors**: Server not ready yet (retryable)
- **Timeouts**: Health check took too long (retryable)  
- **4xx Status Codes**: Unexpected but not fatal (retryable)
- **5xx Status Codes**: Server error (fatal - stops immediately)
- **Max Attempts Exceeded**: Server never became ready (fatal)

Use `--fail-fast` to stop on the first fatal error, or let all servers attempt to start before reporting failures.

## Use Cases

- **Integration Testing**: Start database, backend, and frontend before running tests
- **Development Environment**: Boot all microservices in the correct order
- **CI/CD Pipelines**: Orchestrate service startup with proper health checks
- **E2E Testing**: Ensure all services are healthy before test execution
- **Migration Workflows**: Run migrations before starting dependent services

## Migration from v1.x

Version 2.0 has breaking changes:

- Removed `config` crate dependency (now using `serde_yaml` directly)
- Configuration parsing is stricter
- Error messages have changed
- New required dependencies in Cargo.toml

To migrate:
1. Update `server-runner` to v2.0
2. Test your YAML configurations (v1.x configs should mostly work)
3. Update any scripts that parse error output
4. Take advantage of new features (hooks, dependencies, priorities, etc.)

## License

See LICENSE file for details.

## Contributing

Contributions are welcome! Please open an issue or pull request on GitHub.
