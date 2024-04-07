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

Server Runner will attempt to check a server's status up to ten times with one second between each attempt. If a server is not responding with HTTP 200 after that, Server Runner will shutdown all servers and exit. 
