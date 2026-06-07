use clap::Parser;

#[derive(Parser)]
#[command(version)]
pub struct Args {
    #[arg(short, long, default_value = "servers.yaml")]
    pub config: String,

    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    #[arg(short, long, default_value_t = 10, value_parser = clap::value_parser!(u8).range(1..=255))]
    pub attempts: u8,

    /// Run the interactive TUI control panel instead of plain log output.
    #[arg(long, default_value_t = false)]
    pub tui: bool,
}
