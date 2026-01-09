use clap::Parser;

#[derive(Parser, Debug)]
#[command(version)]
#[command(about = "Runs multiple servers, waits for them to be ready, then executes a command")]
pub struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "servers.yaml")]
    pub config: String,

    /// Enable verbose logging
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    /// Maximum number of connection attempts per server
    #[arg(short, long, default_value_t = 10)]
    pub attempts: u32,

    /// Seconds between health check polls
    #[arg(short, long, default_value_t = 1)]
    pub poll_interval: u64,

    /// Maximum total seconds to wait before giving up
    #[arg(short = 't', long)]
    pub startup_timeout: Option<u64>,

    /// Stop immediately on first server failure
    #[arg(short, long, default_value_t = false)]
    pub fail_fast: bool,

    /// Continue running even if some servers fail
    #[arg(long, default_value_t = false)]
    pub continue_on_error: bool,
}

impl Args {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.attempts == 0 {
            anyhow::bail!("Attempts must be greater than 0");
        }

        if self.poll_interval == 0 {
            anyhow::bail!("Poll interval must be greater than 0");
        }

        if self.fail_fast && self.continue_on_error {
            anyhow::bail!("Cannot use both --fail-fast and --continue-on-error");
        }

        Ok(())
    }
}
