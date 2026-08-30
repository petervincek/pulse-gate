use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use openidconnect::reqwest::Client;

use crate::{
    api::health::health_router,
    app::state::AppState,
    core::config::common::AppConfig,
    model::{connection_postgres::build_postgres_pool, connection_redis::build_redis_pool},
    service::keycloak::KeycloakService,
};

pub mod state;

pub async fn create_app_router(app_config: &AppConfig) -> Result<Router> {
    // create the Redis connection pool
    let redis_pool = build_redis_pool(app_config).await?;

    // create the Postgres connection pool
    let postgres_pool = build_postgres_pool(app_config).await?;

    // create the http client (pool underneath)
    let http_client = Client::builder().build()?;

    // create the keycloak service smart pointer, so it's shareable for the whole app
    let keycloak_service = Arc::new(KeycloakService::new(http_client.clone(), app_config));

    // create application state with shared dependencies
    let app_state = AppState::new(redis_pool, postgres_pool, keycloak_service, "PulseGate");

    // create health router
    let health_router = health_router();

    // create the main/root level application router
    let app_router = Router::new()
        .nest("/health", health_router)
        .with_state(app_state);

    Ok(app_router)
}
