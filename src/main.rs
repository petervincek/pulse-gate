use std::{process, sync::Arc};

use anyhow::Result;
use pulse_gate::{
    app::create_app_router,
    core::{
        config::common::{AppConfig, ConfigManager, LoggingConfig},
        logging::LogManager,
    },
};
use tracing_appender::non_blocking::WorkerGuard;

/// Loads application configuration
fn load_app_config(config_manager: &ConfigManager) -> AppConfig {
    match config_manager.load_or_create() {
        Ok(app_config) => app_config,
        Err(err) => {
            // logging is not yet initialized at this point
            eprintln!("[FATAL ERROR] Failed to load configuration file: {:?}", err);
            process::exit(1);
        }
    }
}

/// Initialize logger and logging framework
fn init_logger(logging_config: &LoggingConfig, log_manager: &LogManager) -> WorkerGuard {
    match log_manager.init(logging_config) {
        Ok(log_guard) => log_guard,
        Err(err) => {
            eprintln!("[FATAL ERROR] Failed to initialize logger: {:?}", err);
            process::exit(1);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // prepare the config manager
    let config_manager = Arc::new(ConfigManager::default());
    // try to load the configuration
    let app_config = load_app_config(&config_manager).merge_with_env();
    // prepare the log manager
    let log_manager = LogManager::new(config_manager.clone());

    // init/setup the logging
    let _log_guard = init_logger(&app_config.logging_config, &log_manager);

    let app_router = create_app_router(&app_config).await?;
    println!("PulseGate starting ...");

    // create a TCP listener
    let server_port = app_config.server_config.port;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{server_port}")).await?;
    println!("PulseGate started at: 127.0.0.1:{server_port}");
    // bind the application to a port listener
    axum::serve(listener, app_router).await?;
    Ok(())
}
