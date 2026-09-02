use std::sync::Arc;

use dashmap::DashMap;
use deadpool_redis::Pool as RedisPool;
use reqwest::Client;
use sqlx::PgPool;

use crate::model::route_target::{PathPrefix, RouteTarget};
use crate::service::keycloak::KeycloakService;

#[derive(Clone)]
pub struct AppState {
    pub redis_pool: RedisPool, // internally this uses smart pointers so it's easily clonable
    pub pg_pool: PgPool,       // internally this uses smart pointers so it's easily clonable
    pub keycloak_service: Arc<KeycloakService>, // smart pointer for keycloak service
    pub service_routes: Arc<DashMap<PathPrefix, RouteTarget>>, // concurrent map for PathPrefix -> RouteTarget
    pub http_client: Client,                                   // smart pointer for http client
    pub service_name: String,
}

impl AppState {
    pub fn new(
        redis_pool: RedisPool,
        pg_pool: PgPool,
        keycloak_service: Arc<KeycloakService>,
        service_routes: Arc<DashMap<PathPrefix, RouteTarget>>,
        http_client: Client,
        service_name: impl Into<String>,
    ) -> Self {
        Self {
            redis_pool,
            pg_pool,
            keycloak_service,
            service_routes,
            http_client,
            service_name: service_name.into(),
        }
    }
}
