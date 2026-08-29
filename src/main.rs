use std::{process, sync::Arc};

use anyhow::Result;
use axum::Router;
use openidconnect::reqwest::Client;
use pulse_gate::{
    api::health::health_router,
    app::state::AppState,
    core::{
        config::common::{AppConfig, ConfigManager, LoggingConfig},
        logging::LogManager,
    },
    model::{connection_postgres::build_postgres_pool, connection_redis::build_redis_pool},
    service::keycloak::KeycloakService,
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
    let app_config = load_app_config(&config_manager);
    // prepare the log manager
    let log_manager = LogManager::new(config_manager.clone());

    // init/setup the logging
    let _log_guard = init_logger(&app_config.logging_config, &log_manager);

    // create the Redis connection pool
    let redis_pool = build_redis_pool(&app_config).await?;

    // create the Postgres connection pool
    let postgres_pool = build_postgres_pool(&app_config).await?;

    // create the http client (pool underneath)
    let http_client = Client::builder().build()?;

    // create the keycloak service smart pointer, so it's shareable for the whole app
    let keycloak_service = Arc::new(KeycloakService::new(http_client.clone(), &app_config));

    // create application state with shared dependencies
    let app_state = AppState::new(redis_pool, postgres_pool, keycloak_service, "PulseGate");

    println!("PulseGate starting ...");

    // create health router
    let health_router = health_router();

    // create the main/root level application router
    let app_router = Router::new()
        .nest("/health", health_router)
        .with_state(app_state);

    // create a TCP listener
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("PulseGate started at: 127.0.0.1:3000");
    // bind the application to a port listener
    axum::serve(listener, app_router).await?;
    Ok(())
}
