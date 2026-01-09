# Server Runner v2.0 - Example Configurations

This directory contains example configuration files demonstrating various Server Runner v2.0 features.

## Examples

### 1. `simple.yaml` - Getting Started
The minimal configuration to get started with Server Runner.

**Use case**: Simple single-server testing

```bash
server-runner -c examples/simple.yaml
```

**Features demonstrated**:
- Basic server configuration
- Health check URL
- Final command execution

---

### 2. `integration-testing.yaml` - Integration Testing
Orchestrate a complete test environment with database, API, and frontend.

**Use case**: E2E testing with multiple services

```bash
server-runner -c examples/integration-testing.yaml -v
```

**Features demonstrated**:
- Priority-based startup
- Lifecycle hooks (migrations, seeding)
- Dependencies between services
- Different output modes (file, capture, null)
- Global environment variables
- Health check configuration

---

### 3. `microservices.yaml` - Complex Microservices
Full microservice architecture with infrastructure, core services, gateway, and workers.

**Use case**: Development environment for microservices

```bash
server-runner -c examples/microservices.yaml --fail-fast
```

**Features demonstrated**:
- Complex dependency graph
- Multiple priority levels (1-4)
- Infrastructure services (PostgreSQL, Redis, RabbitMQ)
- Core services with database migrations
- API Gateway pattern
- Background workers
- Output prefixing for each service
- Custom health check headers

---

### 4. `v2-showcase.yaml` - Feature Showcase
Comprehensive example demonstrating ALL v2.0 features.

**Use case**: Learning and reference

```bash
DB_PASSWORD=secret123 server-runner -c examples/v2-showcase.yaml -v
```

**Features demonstrated**:
- Environment variable expansion (`${VAR}`)
- All health check options (methods, status codes, headers)
- All lifecycle hooks (before_start, after_ready, before_stop)
- All output modes (inherit, capture, file, null)
- Priority ordering
- Explicit dependencies
- Circular dependency detection
- Custom retry intervals
- Startup delays
- Per-server and global env vars

---

## Running the Examples

### Prerequisites

Most examples use Docker and Node.js. Ensure you have:
- Docker (for database/infrastructure services)
- Node.js (for application services)

### Environment Variables

Some examples require environment variables:

```bash
# For v2-showcase.yaml
export DB_PASSWORD="your-password"
export API_TOKEN="your-token"

server-runner -c examples/v2-showcase.yaml
```

### CLI Options

Try different CLI options to see how they affect behavior:

```bash
# Verbose logging
server-runner -c examples/microservices.yaml -v

# Fail fast on errors
server-runner -c examples/integration-testing.yaml --fail-fast

# Custom polling interval
server-runner -c examples/simple.yaml --poll-interval 2

# Startup timeout
server-runner -c examples/microservices.yaml --startup-timeout 120

# More attempts
server-runner -c examples/integration-testing.yaml --attempts 20
```

---

## Customizing Examples

### Modify for Your Stack

These examples use generic commands. Adapt them for your stack:

**Python/Django:**
```yaml
- name: "Django API"
  url: "http://localhost:8000/health/"
  command: "python manage.py runserver"
  hooks:
    before_start: "python manage.py migrate"
```

**Ruby/Rails:**
```yaml
- name: "Rails API"
  url: "http://localhost:3000/health"
  command: "rails server"
  hooks:
    before_start: "rails db:migrate"
```

**Go:**
```yaml
- name: "Go API"
  url: "http://localhost:8080/health"
  command: "./bin/api-server"
  hooks:
    before_start: "go run migrations/main.go"
```

**Rust:**
```yaml
- name: "Rust API"
  url: "http://localhost:3000/health"
  command: "cargo run --release"
```

### Add Real Health Endpoints

The examples assume your services expose `/health` endpoints. If not, use:

```yaml
health_check:
  method: GET
  expected_status: [200, 404]  # Accept 404 if no health endpoint
```

Or create simple health endpoints:

**Express.js:**
```javascript
app.get('/health', (req, res) => res.json({ status: 'ok' }));
```

**Flask:**
```python
@app.route('/health')
def health():
    return {'status': 'ok'}
```

**FastAPI:**
```python
@app.get("/health")
def health():
    return {"status": "ok"}
```

---

## Best Practices

### 1. Use Priorities Wisely
- Priority 1: Infrastructure (databases, caches)
- Priority 2: Core services
- Priority 3: API gateways, workers
- Priority 4: Frontend, optional services

### 2. Add Health Checks
Always implement proper health endpoints that:
- Return 200 when service is ready
- Check database connections
- Verify critical dependencies

### 3. Use Lifecycle Hooks
- `before_start`: Database migrations, setup tasks
- `after_ready`: Data seeding, warm-up requests
- `before_stop`: Backups, graceful shutdown

### 4. Control Output
- Development: `mode: capture` with prefixes
- CI/CD: `mode: file` for log collection
- Verbose services: `mode: null` to reduce noise

### 5. Set Appropriate Timeouts
- Fast APIs: `timeout: 5`
- Databases: `timeout: 10-15`
- Complex services: `timeout: 20-30`

### 6. Use Environment Variables
Keep secrets out of config files:
```yaml
env:
  DB_PASSWORD: "${DB_PASSWORD}"  # From environment
  API_KEY: "${API_KEY}"
```

---

## Troubleshooting

### Service won't start
```bash
# Use verbose mode to see detailed logs
server-runner -c config.yaml -v

# Check if ports are already in use
lsof -i :3000

# Increase timeout for slow services
server-runner -c config.yaml --startup-timeout 180
```

### Health check fails
```bash
# Test health endpoint manually
curl http://localhost:3000/health

# Use less strict health check
health_check:
  expected_status: [200, 404, 503]  # More permissive
```

### Circular dependencies
```
Error: Circular dependency detected involving server 'API': API -> Database -> API
```

Fix by removing the circular reference or adjusting priorities.

---

## More Examples

Want to contribute an example? Open a PR with:
- YAML configuration file
- Brief description of use case
- Any special requirements

---

## Resources

- **Main Documentation**: ../README.md
- **Release Notes**: ../RELEASE_NOTES_v2.0.md
- **Migration Guide**: ../CHANGELOG.md

Happy orchestrating! 🚀
