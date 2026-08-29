use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use deadpool_redis::redis::AsyncCommands;
use serde_json::{Value, json};
use tokio::join;

use crate::{app::state::AppState, model::connection_postgres::validate_postgres_startup};

/// `get_health` is a handler function that receives through Axum extractor dependency for global
/// state `AppState`
pub async fn get_health(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (redis_result, postgres_result) =
        join!(check_redis_health(&state), check_postgres_health(&state));

    let redis_status = match &redis_result {
        Ok(_) => "ok",
        Err(_) => "error",
    };
    let postgres_status = match &postgres_result {
        Ok(_) => "ok",
        Err(_) => "error",
    };

    let overall_status = if redis_status == "ok" && postgres_status == "ok" {
        "ok"
    } else {
        "degraded"
    };

    let body = json!({
        "status": overall_status,
        "service": state.service_name,
        "redis": redis_status,
        "postgres": postgres_status,
        "errors": {
            "redis": redis_result.err(),
            "postgres": postgres_result.err(),
        }
    });

    if overall_status == "ok" {
        Ok(Json(body))
    } else {
        Err((StatusCode::SERVICE_UNAVAILABLE, Json(body)))
    }
}

async fn check_redis_health(state: &AppState) -> Result<(), String> {
    let mut conn = state
        .redis_pool
        .get()
        .await
        .map_err(|err| err.to_string())?;
    let pong: String = conn.ping().await.map_err(|err| err.to_string())?;

    if pong == "PONG" {
        Ok(())
    } else {
        Err(format!("unexpected redis reply: {}", pong))
    }
}

async fn check_postgres_health(state: &AppState) -> Result<(), String> {
    validate_postgres_startup(&state.pg_pool)
        .await
        .map_err(|err| err.to_string())
}

/// `health_router` function creates the router instance we want to define our
/// health check endpoint in, the instance is type as `Router<AppState>` as
/// it's need shared dependencies from the global state
pub fn health_router() -> Router<AppState> {
    Router::new().route("/", get(get_health))
}
