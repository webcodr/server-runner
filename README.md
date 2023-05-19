# Server Runner

Server Runner is a little Rust programm to run multiple web servers, check until all servers are ready via a URL that returns HTTP 200 und runs a command when all servers are ready.

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

Default name of the config file is `servers.yaml`.

