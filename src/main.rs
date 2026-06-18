mod cli;
mod config;
mod core;
mod runner;

use clap::Parser;

use cli::Args;

fn exit_with_error(e: anyhow::Error) -> ! {
    eprintln!("An error occurred: {}", e);
    std::process::exit(1);
}

fn main() {
    let args = Args::parse();

    let log_level = if args.verbose {
        simplelog::LevelFilter::Info
    } else {
        simplelog::LevelFilter::Warn
    };
    let _ = simplelog::TermLogger::init(
        log_level,
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    );

    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|rt| rt.block_on(async_main(args)));

    if let Err(e) = result {
        exit_with_error(e);
    }
}

async fn async_main(args: Args) -> anyhow::Result<()> {
    let config = config::get_config(&args.config)?;

    if args.tui {
        runner::tui::run(config, args.attempts).await
    } else {
        runner::plain::run(config, args.attempts).await
    }
}
