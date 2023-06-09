# Server Runner

[![Tests](https://github.com/webcodr/server-runner/actions/workflows/test.yml/badge.svg)](https://github.com/webcodr/server-runner/actions/workflows/test.yml)

Server Runner is a little Rust programm to run multiple web servers, check until all servers are ready via a URL that returns HTTP 200 und runs a command when all servers are ready.

## Installation

Currently Server Runner is only available via Cargo. It will be also available
via NPM in the near future, since NPM is available on almost any OS out there
and it's much easier to publish than to many other package managers.

### Installation via Cargo

~~~ sh
cargo install server-runner
~~~

## Configuration File

Example

~~~ yaml
servers:
    - name: "My web server"
      url: "http://localhost:8080"
      command: "node webserver.js"
command: "node cypress"
~~~

~~~ sh
server-runner -c config.yaml
~~~

Default name of the config file is `servers.yaml` in your current working directory.
