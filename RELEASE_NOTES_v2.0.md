# Server Runner v2.0.0 Release Notes

## 🎉 Major Release - Breaking Changes

Version 2.0.0 is a complete rewrite of Server Runner with significant architectural improvements and powerful new features. This release focuses on flexibility, reliability, and enterprise-grade orchestration capabilities.

## ⚠️ Breaking Changes

**This is a major version with breaking changes. Please review carefully before upgrading.**

### Configuration Changes
- Switched from `config` crate to `serde_yaml` for YAML parsing
- Configuration validation is now stricter
- Error messages have changed format and content
- Some edge cases in YAML parsing may behave differently

### Dependency Changes
- **Removed**: `config = "0.15.11"`
- **Added**: `serde_yaml = "0.9"`, `regex = "1.11.1"`

### CLI Changes
- `--attempts` parameter now accepts u32 instead of u8 (breaking for type-strict integrations)

### Behavioral Changes
- Servers are now started in priority order (lower priority numbers start first)
- Health check failures with 5xx status codes are now immediately fatal
- Default behavior distinguishes between retryable and fatal errors
- Process output handling has changed (though default behavior is preserved)

## 🚀 New Features

### 1. Modular Architecture
- Complete code reorganization into focused modules:
  - `cli.rs` - Command-line interface
  - `config.rs` - Configuration management
  - `server.rs` - Server orchestration
  - `process.rs` - Process management
- Improved code maintainability and testability
- Better separation of concerns

### 2. Advanced Health Checks
Configure sophisticated health check strategies:

```yaml
servers:
  - name: "API"
    url: "http://localhost:3000/health"
    command: "node api.js"
    health_check:
      method: POST                    # GET, POST, HEAD, PUT, PATCH, DELETE
      expected_status: [200, 204]     # Accept multiple success codes
      headers:
        Authorization: "Bearer token"
        Content-Type: "application/json"
```

### 3. Environment Variable Support
Use environment variables anywhere in your configuration:

```yaml
servers:
  - name: "Database"
    url: "http://localhost:${DB_PORT}/health"
    command: "postgres -D ${DATA_DIR}"
    env:
      POSTGRES_USER: "${DB_USER}"
      POSTGRES_PASSWORD: "${DB_PASS}"
```

### 4. Lifecycle Hooks
Execute commands at key points in the server lifecycle:

```yaml
servers:
  - name: "Database"
    url: "http://localhost:5432/health"
    command: "postgres -D data"
    hooks:
      before_start: "npm run db:migrate"   # Run migrations
      after_ready: "npm run db:seed"       # Seed data when ready
      before_stop: "npm run db:backup"     # Backup before shutdown
```

### 5. Dependency Management
Control startup order with priorities and explicit dependencies:

```yaml
servers:
  - name: "Database"
    url: "http://localhost:5432/health"
    command: "postgres -D data"
    priority: 1                  # Start first

  - name: "Cache"
    url: "http://localhost:6379/health"
    command: "redis-server"
    priority: 1                  # Start with database

  - name: "API"
    url: "http://localhost:3000/health"
    command: "node api.js"
    priority: 2                  # Start after priority 1
    depends_on: ["Database"]     # Explicit dependency
```

**Features:**
- Automatic startup ordering by priority level
- Circular dependency detection
- Validation of dependency relationships

### 6. Process Output Control
Fine-grained control over server output:

```yaml
servers:
  - name: "API"
    url: "http://localhost:3000/health"
    command: "node api.js"
    output:
      mode: "capture"           # inherit, capture, null, file
      prefix: "[API]"           # Prefix output lines
      stdout: "logs/api.out"    # For file mode
      stderr: "logs/api.err"    # For file mode
```

**Modes:**
- `inherit` - Pass through to terminal (default)
- `capture` - Capture and optionally prefix
- `null` - Discard all output
- `file` - Write to log files

### 7. Enhanced CLI Options

```bash
server-runner \
  --config servers.yaml \
  --poll-interval 2 \           # Custom polling interval
  --startup-timeout 120 \        # Total timeout
  --fail-fast \                  # Stop on first failure
  --attempts 50                  # Higher attempt limits
```

**New Options:**
- `-p, --poll-interval <SECONDS>` - Seconds between health check polls
- `-t, --startup-timeout <SECONDS>` - Maximum total startup time
- `-f, --fail-fast` - Exit on first server failure
- `--continue-on-error` - Continue despite failures

### 8. Intelligent Error Handling

Server Runner now distinguishes between error types:

| Error Type | Behavior | Retryable |
|------------|----------|-----------|
| Connection refused | Server starting | ✅ Yes |
| Timeout | Server slow | ✅ Yes |
| 4xx status | Unexpected but continue | ✅ Yes |
| 5xx status | Server broken | ❌ No - Fatal |
| Max attempts | Never ready | ❌ No - Fatal |

### 9. Additional Configuration Options

```yaml
servers:
  - name: "API"
    url: "http://localhost:3000/health"
    command: "node api.js"
    timeout: 10              # HTTP request timeout (seconds)
    retry_interval: 2        # Seconds between checks
    startup_delay: 5         # Wait before first check
```

### 10. Global Environment Variables

```yaml
env:
  NODE_ENV: "production"
  LOG_LEVEL: "info"

servers:
  - name: "API"
    url: "http://localhost:3000"
    command: "node api.js"
    env:
      PORT: "3000"           # Server-specific env vars

command: "npm test"
```

## 📈 Improvements

### Reliability
- Better error messages with actionable context
- Stricter configuration validation catches issues early
- Circular dependency detection prevents deadlocks
- Improved process cleanup on shutdown

### Performance
- More efficient health check polling
- Reduced unnecessary retries for fatal errors
- Optimized dependency resolution

### Developer Experience
- Comprehensive error messages
- Detailed logging with `-v` flag
- Better documentation and examples
- Clearer configuration validation errors

## 📝 Migration Guide

### Step 1: Update Installation

```bash
cargo install server-runner --version 2.0.0
```

### Step 2: Review Your Configuration

Most v1.x configurations will work without changes, but test thoroughly:

```bash
# Test your config
server-runner -c your-config.yaml -v --fail-fast
```

### Step 3: Update Error Handling

If your scripts parse error output, update for new error formats:

**v1.x:**
```
Could not find config file servers.yaml
```

**v2.0:**
```
An error occurred: Could not find config file servers.yaml
```

### Step 4: Leverage New Features

Consider adding:
- Lifecycle hooks for database migrations
- Dependencies between related services
- Output handling for cleaner logs
- Environment variable expansion for secrets

### Example Migration

**Before (v1.x):**
```yaml
servers:
  - name: "API"
    url: "http://localhost:3000"
    command: "node api.js"
    timeout: 5

command: "npm test"
```

**After (v2.0) - Enhanced:**
```yaml
env:
  NODE_ENV: "test"

servers:
  - name: "API"
    url: "http://localhost:3000/health"
    command: "node api.js"
    timeout: 5
    retry_interval: 1
    health_check:
      method: GET
      expected_status: [200]
    hooks:
      before_start: "npm run db:migrate"
    output:
      mode: "capture"
      prefix: "[API]"
    env:
      PORT: "3000"

command: "npm test"
```

## 🐛 Known Issues

- Integration test `fails_on_multiple_unreachable_servers` is flaky in CI environments (works in manual testing)
- Some YAML edge cases may parse differently due to `serde_yaml` vs `config` crate

## 🔮 Future Roadmap

Potential features for v2.1+:
- Exponential backoff for health checks
- Parallel server startup within same priority level
- Health check retry strategies
- Signal handling improvements (SIGTERM grace periods)
- Server restart on failure
- Metrics and monitoring endpoints

## 💡 Use Cases

### Integration Testing
```yaml
servers:
  - name: "PostgreSQL"
    priority: 1
    hooks:
      before_start: "docker-compose up -d postgres"
      after_ready: "npm run migrate"
  
  - name: "Redis"
    priority: 1
    hooks:
      before_start: "docker-compose up -d redis"
  
  - name: "Backend"
    priority: 2
    depends_on: ["PostgreSQL", "Redis"]
  
  - name: "Frontend"
    priority: 3
    depends_on: ["Backend"]

command: "npm run test:e2e"
```

### Development Environment
```yaml
servers:
  - name: "API"
    output:
      mode: "capture"
      prefix: "[API]"
  
  - name: "Worker"
    output:
      mode: "file"
      stdout: "logs/worker.log"
      stderr: "logs/worker.err"

command: "npm run dev"
```

## 🙏 Acknowledgments

Thank you to all contributors and users who provided feedback that shaped v2.0!

## 📞 Support

- **Issues**: https://github.com/webcodr/server-runner/issues
- **Documentation**: https://github.com/webcodr/server-runner
- **Crate**: https://crates.io/crates/server-runner

## 📄 License

See LICENSE file for details.

---

**Full Changelog**: v1.6.0...v2.0.0
