use anyhow::Context;
use clap::Parser;
use log::info;
use std::sync::{Arc, Mutex};

mod attempts;
mod command;
mod config;
mod constants;
mod server_management;
mod tui;

use attempts::Attempts;
use config::{get_config, get_config_with_logging, Config};
use constants::{DEFAULT_CONFIG_FILE, DEFAULT_MAX_ATTEMPTS};
use server_management::{start_servers, stop_servers, wait_for_servers, execute_command};
use tui::TuiApp;

#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(short, long, default_value_t = String::from(DEFAULT_CONFIG_FILE))]
    config: String,

    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    #[arg(short, long, default_value_t = DEFAULT_MAX_ATTEMPTS)]
    attempts: u8,

    #[arg(long, default_value_t = false)]
    tui: bool,
}


fn run(args: Args) -> anyhow::Result<()> {
    let config = if args.tui {
        get_config_with_logging(&args.config, false)?
    } else {
        get_config(&args.config)?
    };
    
    let log_level = if args.verbose {
        simplelog::LevelFilter::Info
    } else {
        simplelog::LevelFilter::Warn
    };

    simplelog::TermLogger::init(
        log_level,
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    )?;

    if args.tui {
        let mut app = TuiApp::new(config);
        app.run()?;
    } else {
        run_cli_mode(config, args.attempts)?;
    }

    Ok(())
}

fn run_cli_mode(config: Config, max_attempts: u8) -> anyhow::Result<()> {
    let server_processes = start_servers(&config.servers, true)?;
    let server_processes_arc_mutex = Arc::new(Mutex::new(server_processes));
    let server_processes_clone = Arc::clone(&server_processes_arc_mutex);

    ctrlc::set_handler(move || {
        let mut processes = server_processes_clone.lock();
        
        if let Err(e) = stop_servers(&mut processes) {
            exit_with_error(anyhow::anyhow!("Error stopping servers: {}", e));
        }
        
        info!("All servers stopped successfully");
        std::process::exit(0);
    })?;

    let attempts = Attempts::new(max_attempts);
    match wait_for_servers(&config.servers, attempts, true) {
        Ok(_) => {
            info!("Running command {}", config.command);
            let output = execute_command(&config.command)
                .context(format!("Could not start process {}", config.command))?;
            
            if output.status.success() {
                info!("Command {} finished successfully", config.command);
            } else {
                eprintln!("Command {} failed with exit code: {:?}", config.command, output.status.code());
            }
        }
        Err(e) => {
            stop_servers(&mut server_processes_arc_mutex.lock())?;
            return Err(e);
        }
    }

    stop_servers(&mut server_processes_arc_mutex.lock())?;
    Ok(())
}

fn exit_with_error(e: anyhow::Error) -> ! {
    eprintln!("An error occurred: {}", e);
    std::process::exit(1)
}

fn main() {
    let args = Args::parse();

    if let Err(e) = run(args) {
        exit_with_error(e);
    }
}