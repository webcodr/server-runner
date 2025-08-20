pub const DEFAULT_CONFIG_FILE: &str = "servers.yaml";
pub const DEFAULT_MAX_ATTEMPTS: u8 = 10;
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 5;
pub const HEALTH_CHECK_INTERVAL_SECONDS: u64 = 1;
pub const MAX_OUTPUT_LINES_PER_SERVER: usize = 5;

#[cfg(windows)]
pub const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;