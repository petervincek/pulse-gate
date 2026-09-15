use axum::{Router, middleware, routing::any};

use crate::{
    api::{
        middleware::authentication::require_valid_token,
        reverse_proxy::handler::dynamic_proxy_handler,
    },
    app::state::AppState,
};

pub mod handler;

/// `dynamic_proxy_router`, wildcard path route captures all incoming HTTP methods and paths
pub fn dynamic_proxy_router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/{*path}", any(dynamic_proxy_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_valid_token,
        ))
        .with_state(state)
}
