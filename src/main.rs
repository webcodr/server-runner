mod cli;
mod config;
mod process;
mod server;

use clap::Parser;
use cli::Args;
use config::load_config;
use log::info;
use server::ServerManager;

fn run(args: Args) -> anyhow::Result<()> {
    args.validate()?;

    let config = load_config(&args.config)?;
    let mut manager = ServerManager::new(config);

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

    // Set up Ctrl+C handler
    let processes_arc = manager.get_processes_arc();
    let server_configs = manager.config.servers.clone();

    ctrlc::set_handler(move || {
        let mut processes = processes_arc.lock().unwrap();

        match server::stop_servers(&mut processes, &server_configs) {
            Ok(_) => info!("All servers stopped successfully"),
            Err(e) => {
                eprintln!("Error stopping servers: {}", e);
                std::process::exit(1);
            }
        };

        std::process::exit(0);
    })?;

    // Handle startup timeout
    if let Some(timeout) = args.startup_timeout {
        let processes_arc_clone = manager.get_processes_arc();
        let server_configs_clone = manager.config.servers.clone();

        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(timeout));
            eprintln!("Startup timeout of {} seconds exceeded", timeout);

            let mut processes = processes_arc_clone.lock().unwrap();
            let _ = server::stop_servers(&mut processes, &server_configs_clone);

            std::process::exit(1);
        });
    }

    // Start servers with priority ordering
    manager.start_servers(args.attempts, args.poll_interval, args.fail_fast)?;

    // Run the final command
    manager.run_final_command()?;

    // Stop all servers
    manager.stop_all_servers()?;

    Ok(())
}

fn exit_with_error(e: anyhow::Error) -> ! {
    eprintln!("An error occurred: {}", e);
    std::process::exit(1)
}

fn main() {
    let args = Args::parse();

    match run(args) {
        Ok(_) => {}
        Err(e) => exit_with_error(e),
    }
}
