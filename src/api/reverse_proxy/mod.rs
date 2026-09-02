use axum::{Router, routing::any};

use crate::{api::reverse_proxy::handler::dynamic_proxy_handler, app::state::AppState};

pub mod handler;

/// `dynamic_proxy_router`, wildcard path route captures all incoming HTTP methods and paths
pub fn dynamic_proxy_router() -> Router<AppState> {
    Router::new().route("/api/v1/{*path}", any(dynamic_proxy_handler))
}
