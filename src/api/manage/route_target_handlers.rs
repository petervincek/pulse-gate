use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use crate::{
    app::state::AppState,
    model::{route_target::RouteTarget, route_target_event::RouteTargetEvent},
};

fn normalize_path_prefix(path_prefix: &str) -> String {
    let trimmed = path_prefix.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    }
}

fn validate_path_prefix(path_prefix: &str) -> Result<(), String> {
    let candidate = path_prefix.trim();

    if candidate.is_empty() {
        return Err("path_prefix must not be empty".to_string());
    }

    if !candidate.starts_with('/') {
        return Err("path_prefix must start with '/'".to_string());
    }

    if candidate.chars().any(char::is_whitespace)
        || candidate.contains('?')
        || candidate.contains('#')
        || candidate.contains("//")
    {
        return Err("path_prefix contains invalid characters or whitespace".to_string());
    }

    Ok(())
}

fn validate_upstream_base_url(upstream_base_url: &str) -> Result<(), String> {
    let candidate = upstream_base_url.trim();

    if candidate.is_empty() {
        return Err("upstream_base_url must not be empty".to_string());
    }

    if !candidate.starts_with("http://") && !candidate.starts_with("https://") {
        return Err("upstream_base_url must be an absolute http(s) URL".to_string());
    }

    candidate
        .parse::<axum::http::Uri>()
        .map_err(|_| "upstream_base_url is not a valid http(s) URL".to_string())?;

    Ok(())
}

fn validate_route_target_request(payload: &RouteTargetRequest) -> Result<(), String> {
    validate_path_prefix(&payload.path_prefix)?;
    validate_upstream_base_url(&payload.upstream_base_url)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteTargetRequest {
    pub path_prefix: String,
    pub upstream_base_url: String,
    pub rate_limit_per_min: i32,
    pub required_role: Option<String>,
}

impl From<RouteTargetRequest> for RouteTarget {
    fn from(value: RouteTargetRequest) -> Self {
        RouteTarget::new(
            value.path_prefix,
            value.upstream_base_url,
            value.rate_limit_per_min,
            value.required_role,
        )
    }
}

pub async fn list_route_targets(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let route_targets = state
        .route_target_repo
        .list_route_targets()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok((StatusCode::OK, Json(route_targets)))
}

pub async fn get_route_target(
    State(state): State<AppState>,
    Path(path_prefix): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let normalized_path_prefix = normalize_path_prefix(&path_prefix);
    validate_path_prefix(&normalized_path_prefix).map_err(|err| (StatusCode::BAD_REQUEST, err))?;

    let route_target = state
        .route_target_repo
        .get_route_target_by_path_prefix(&normalized_path_prefix)
        .await
        .map_err(|err| (StatusCode::NOT_FOUND, err.to_string()))?;

    Ok((StatusCode::OK, Json(route_target)))
}

pub async fn create_route_target(
    State(state): State<AppState>,
    Json(payload): Json<RouteTargetRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    validate_route_target_request(&payload).map_err(|err| (StatusCode::BAD_REQUEST, err))?;

    let route_target = RouteTarget::from(payload);

    let created = state
        .route_target_repo
        .create_route_target(route_target.clone())
        .await
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;

    state
        .service_routes
        .insert(created.path_prefix.clone(), created.clone());
    let route_version = state
        .route_target_event_repo
        .next_route_version(&created.path_prefix)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    state
        .route_target_event_repo
        .publish_route_event(&RouteTargetEvent::created(created.clone(), route_version))
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn update_route_target(
    State(state): State<AppState>,
    Path(path_prefix): Path<String>,
    Json(payload): Json<RouteTargetRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if normalize_path_prefix(&path_prefix) != payload.path_prefix {
        return Err((
            StatusCode::BAD_REQUEST,
            "invalid path_prefix, the value in the path and in the json body are not the same"
                .to_string(),
        ));
    }
    validate_route_target_request(&payload).map_err(|err| (StatusCode::BAD_REQUEST, err))?;
    let route_target = RouteTarget::new(
        payload.path_prefix,
        payload.upstream_base_url,
        payload.rate_limit_per_min,
        payload.required_role,
    );

    let updated = state
        .route_target_repo
        .update_route_target(route_target.clone())
        .await
        .map_err(|err| (StatusCode::NOT_FOUND, err.to_string()))?;

    state
        .service_routes
        .insert(updated.path_prefix.clone(), updated.clone());
    let route_version = state
        .route_target_event_repo
        .next_route_version(&updated.path_prefix)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    state
        .route_target_event_repo
        .publish_route_event(&RouteTargetEvent::updated(updated.clone(), route_version))
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok((StatusCode::OK, Json(updated)))
}

pub async fn delete_route_target(
    State(state): State<AppState>,
    Path(path_prefix): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let normalized_path_prefix = normalize_path_prefix(&path_prefix);
    validate_path_prefix(&normalized_path_prefix).map_err(|err| (StatusCode::BAD_REQUEST, err))?;

    state
        .route_target_repo
        .delete_route_target_by_path_prefix(&normalized_path_prefix)
        .await
        .map_err(|err| (StatusCode::NOT_FOUND, err.to_string()))?;

    state.service_routes.remove(&normalized_path_prefix);
    let route_version = state
        .route_target_event_repo
        .next_route_version(&normalized_path_prefix)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    state
        .route_target_event_repo
        .publish_route_event(&RouteTargetEvent::deleted(
            normalized_path_prefix.clone(),
            route_version,
        ))
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}
