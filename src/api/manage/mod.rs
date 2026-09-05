use axum::{
    Router,
    routing::get,
};

use crate::app::state::AppState;

pub mod route_target_handlers;

use route_target_handlers::{
    create_route_target, delete_route_target, get_route_target, list_route_targets,
    update_route_target,
};

pub fn manage_router() -> Router<AppState> {
    Router::new()
        .route(
            "/route-targets",
            get(list_route_targets).post(create_route_target),
        )
        .route(
            "/route-targets/{path_prefix}",
            get(get_route_target)
                .put(update_route_target)
                .delete(delete_route_target),
        )
}
