use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use deadpool_redis::redis::AsyncCommands;
use serde_json::{Value, json};

use crate::app::state::AppState;

/// `get_health` is a handler function that receives through Axum extractor dependency for global
/// state `AppState`
pub async fn get_health(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // through the shared dependency to global state access the redis pool and get a redis connection
    let mut conn = state.redis_pool.get().await.map_err(|err| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "error",
                "message": err.to_string(),
            })),
        )
    })?;

    // execute the health check - in this case the ping to redis server
    let pong: String = conn.ping().await.map_err(|err| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "error",
                "message": err.to_string(),
            })),
        )
    })?;

    // check the response from the redis server
    if pong != "PONG" {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "error",
                "message": format!("unexpected redis reply: {}", pong),
            })),
        ));
    }

    // everything is fine/ok at this point
    Ok(Json(json!({
        "status": "ok",
        "service": state.service_name,
        "redis": "ok",
    })))
}

/// `health_router` function creates the router instance we want to define our
/// health check endpoint in, the instance is type as `Router<AppState>` as
/// it's need shared dependencies from the global state
pub fn health_router() -> Router<AppState> {
    Router::new().route("/", get(get_health))
}
