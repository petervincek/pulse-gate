use std::sync::Arc;

use dashmap::DashMap;
use deadpool_redis::Pool as RedisPool;
use reqwest::Client;
use sqlx::PgPool;

use crate::{
    api::middleware::authentication::AuthService,
    model::{
        call_stats::CallStatsRepo,
        route_target::{PathPrefix, RouteTarget, RouteTargetRepo},
        route_target_event::RouteTargetEventRepo,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub redis_pool: RedisPool, // internally this uses smart pointers so it's easily clonable
    pub pg_pool: PgPool,       // internally this uses smart pointers so it's easily clonable
    pub keycloak_service: Arc<dyn AuthService>, // smart pointer for auth/keycloak service
    pub route_target_repo: Arc<RouteTargetRepo>, // shared repository for CRUD operations over RouteTarget
    pub route_target_event_repo: Arc<RouteTargetEventRepo>,
    pub call_stats_repo: Arc<CallStatsRepo>, // shared repository for Redis-based call and rate-limit tracking
    pub service_routes: Arc<DashMap<PathPrefix, RouteTarget>>, // concurrent map for PathPrefix -> RouteTarget
    pub http_client: Client,                                   // smart pointer for http client
    pub service_name: String,
}

impl AppState {
    pub fn new(
        redis_pool: RedisPool,
        pg_pool: PgPool,
        keycloak_service: Arc<dyn AuthService>,
        route_target_repo: Arc<RouteTargetRepo>,
        route_target_event_repo: Arc<RouteTargetEventRepo>,
        call_stats_repo: Arc<CallStatsRepo>,
        service_routes: Arc<DashMap<PathPrefix, RouteTarget>>,
        http_client: Client,
        service_name: impl Into<String>,
    ) -> Self {
        Self {
            redis_pool,
            pg_pool,
            keycloak_service,
            route_target_repo,
            route_target_event_repo,
            call_stats_repo,
            service_routes,
            http_client,
            service_name: service_name.into(),
        }
    }
}
