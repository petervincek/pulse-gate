use axum::{Router, http::StatusCode, routing::get};

pub async fn get_health() -> Result<StatusCode, String> {
    Ok(StatusCode::OK)
}

pub fn health_router() -> Router {
    Router::new().route("/", get(get_health))
}
