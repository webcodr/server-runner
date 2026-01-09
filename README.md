# Server Runner

![GitHub](https://img.shields.io/badge/github-webcodr/server--runner-8da0cb?style=for-the-badge&logo=github&labelColor=555555)
![Crates.io Version](https://img.shields.io/crates/v/server-runner?style=for-the-badge&logo=rust&color=fc8d62)
![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/webcodr/server-runner/build.yml?style=for-the-badge)

Server Runner is a little Rust programm to run multiple web servers, check until all servers are ready via a URL that returns HTTP 200 und runs a command when all servers are ready.

## Installation

Currently Server Runner is only available via Cargo. It will be also available
via NPM in the near future, since NPM is available on almost any OS out there
and it's much easier to publish than to many other package managers.

### Installation via Cargo

~~~ sh
cargo install server-runner
~~~

## Usage

~~~ sh
server-runner [OPTIONS]
~~~

### Options

- `-c, --config <FILE>` - Path to configuration file (default: `servers.yaml`)
- `-v, --verbose` - Enable verbose logging
- `-a, --attempts <NUMBER>` - Maximum number of connection attempts per server (default: 10)
- `-h, --help` - Print help information
- `-V, --version` - Print version information

### Example

~~~ sh
server-runner -c config.yaml -v -a 15
~~~

## Configuration File

The configuration file is written in YAML format and defines the servers to start and the command to run when all servers are ready.

### Example Configuration

~~~ yaml
servers:
  - name: "My web server"
    url: "http://localhost:8080"
    command: "node webserver.js"
    timeout: 5  # Optional: HTTP request timeout in seconds (default: 5)
  
  - name: "API server"
    url: "http://localhost:3000/health"
    command: "python api_server.py"
    timeout: 10

command: "npm test"
~~~

### Configuration Fields

**servers** (required): List of servers to start and monitor

Each server requires:
- `name`: Display name for the server
- `url`: HTTP endpoint to check for availability (must return HTTP 200 when ready)
- `command`: Shell command to start the server
- `timeout`: (optional) HTTP request timeout in seconds (default: 5)

**command** (required): Command to execute when all servers are ready

## How It Works

Server Runner will:

1. Start all configured servers simultaneously
2. Poll each server's URL every second until it returns HTTP 200
3. Retry up to the maximum number of attempts (default: 10, configurable with `-a`)
4. Execute the specified command once all servers are ready
5. Shut down all servers when the command completes or if any error occurs

If any server fails to respond with HTTP 200 after the maximum attempts, Server Runner will shut down all servers and exit with an error.
