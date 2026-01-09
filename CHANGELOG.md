# Changelog

All notable changes to Server Runner will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.0.0] - 2024-01-09

### ⚠️ Breaking Changes

- Complete rewrite with modular architecture
- Switched from `config` crate to `serde_yaml` for configuration parsing
- Changed CLI argument `--attempts` type from u8 to u32
- Modified error message formats
- Updated behavior for 5xx HTTP status codes (now immediately fatal)

### Added

#### Configuration
- Environment variable expansion with `${VAR_NAME}` syntax
- Global and per-server environment variables
- Advanced health check options:
  - Custom HTTP methods (GET, POST, HEAD, PUT, PATCH, DELETE)
  - Multiple acceptable status codes
  - Custom request headers
- Server lifecycle hooks:
  - `before_start` - Run before starting server
  - `after_ready` - Run after health check passes
  - `before_stop` - Run before stopping server
- Dependency management:
  - Priority-based startup ordering
  - Explicit `depends_on` relationships
  - Circular dependency detection
- Per-server configuration:
  - `retry_interval` - Custom polling intervals
  - `startup_delay` - Delay before first health check
  - `output` - Process output handling

#### CLI
- `-p, --poll-interval` - Seconds between health check polls
- `-t, --startup-timeout` - Maximum total startup time
- `-f, --fail-fast` - Exit on first server failure
- `--continue-on-error` - Continue despite failures

#### Process Management
- Output handling modes:
  - `inherit` - Pass through to terminal (default)
  - `capture` - Capture with optional prefix
  - `null` - Discard output
  - `file` - Write to log files
- Output prefixing for captured logs
- Separate stdout/stderr file destinations

### Changed

- Refactored codebase into focused modules (cli, config, server, process)
- Improved error handling with distinction between retryable and fatal errors
- Enhanced configuration validation with detailed error messages
- Better process cleanup on shutdown
- More informative logging output

### Improved

- Configuration validation catches more issues early
- Error messages provide better context and actionable information
- Health check logic distinguishes connection errors from server errors
- Documentation with comprehensive examples

### Fixed

- Better handling of server startup failures
- Improved process cleanup when Ctrl+C is pressed
- More reliable health check timeout handling

### Dependencies

- Added: `serde_yaml = "0.9"`
- Added: `regex = "1.11.1"`
- Removed: `config = "0.15.11"`
- Updated: Various dependency versions

### Migration Notes

See [RELEASE_NOTES_v2.0.md](RELEASE_NOTES_v2.0.md) for detailed migration guide.

Most v1.x configurations will work with minimal changes. Key differences:
1. Error message formats have changed
2. Some YAML parsing edge cases may differ
3. 5xx status codes now cause immediate failure
4. New optional configuration fields available

## [1.6.0] - Previous Release

See git history for v1.x changelog.

---

[2.0.0]: https://github.com/webcodr/server-runner/compare/v1.6.0...v2.0.0
[1.6.0]: https://github.com/webcodr/server-runner/releases/tag/v1.6.0
