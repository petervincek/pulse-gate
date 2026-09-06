use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use dashmap::DashMap;
use openidconnect::reqwest::Client;
use reqwest::Client as ReqwestClient;

use crate::{
    api::{health::health_router, manage::manage_router, reverse_proxy::dynamic_proxy_router},
    app::state::AppState,
    core::config::common::AppConfig,
    model::{
        call_stats::CallStatsRepo,
        connection_postgres::build_postgres_pool,
        connection_redis::build_redis_pool,
        route_target::{PathPrefix, RouteTarget, RouteTargetRepo},
        route_target_event::RouteTargetEventRepo,
    },
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
    let mut keycloak_service = KeycloakService::new(http_client.clone(), app_config);
    if let Some(expected_audience) = &app_config.keycloak_config.expected_audience {
        keycloak_service.set_expected_audience(expected_audience);
    }
    let keycloak_service = Arc::new(keycloak_service);

    // warm the cache once at startup so the first request is not blocked on discovery
    if keycloak_service.get_jwks().await.is_ok() {
        // no-op: cache initialized successfully
    }

    // refresh the JWKS in the background so rotated Keycloak keys are picked up without downtime
    let refresh_keycloak_service = keycloak_service.clone();
    refresh_keycloak_service.start_jwks_refresh_loop();

    // create route target repository and load the registered routes from the DB
    let route_target_repo = Arc::new(RouteTargetRepo::new(postgres_pool.clone()));
    let service_routes: Arc<DashMap<PathPrefix, RouteTarget>> = Arc::new(
        route_target_repo
            .list_route_targets()
            .await?
            .into_iter()
            .map(|route_target| (route_target.path_prefix.clone(), route_target))
            .collect(),
    );
    let route_versions: Arc<DashMap<PathPrefix, u64>> = Arc::new(DashMap::new());

    // create the Redis-backed route-target event repository and hydrate the baseline version state
    let route_target_event_repo = Arc::new(RouteTargetEventRepo::new(redis_pool.clone()));
    route_target_event_repo
        .hydrate_route_versions(&service_routes, &route_versions)
        .await?;

    // subscribe to route events only after the startup baseline has been hydrated
    route_target_event_repo
        .start_route_change_listener(
            service_routes.clone(),
            route_versions,
            &app_config.redis_config.url,
        )
        .await?;

    // create the calls stats repo (backed by Redis)
    let call_stats_repo = Arc::new(CallStatsRepo::new(redis_pool.clone()));

    // create http client
    let http_client = ReqwestClient::default();

    // create application state with shared dependencies
    let app_state = AppState::new(
        redis_pool,
        postgres_pool,
        keycloak_service,
        route_target_repo,
        route_target_event_repo,
        call_stats_repo,
        service_routes,
        http_client,
        "PulseGate",
    );

    // create health router
    let health_router = health_router();
    let manage_router = manage_router(app_state.clone());

    // create the dynamic proxy router (responsible for dispatching the incoming requests and streaming back the responses)
    let dynamic_proxy_router = dynamic_proxy_router(app_state.clone());

    // create the main/root level application router
    let app_router = Router::new()
        .nest("/health", health_router)
        .nest("/manage", manage_router)
        .merge(dynamic_proxy_router)
        .with_state(app_state);

    Ok(app_router)
}
