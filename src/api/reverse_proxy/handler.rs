use axum::{Extension, body::Body, extract::State, http, response::Response};
use hyper::{HeaderMap, Method, StatusCode, Uri};
use std::time::Instant;
use tracing::{debug, warn};

use crate::{app::state::AppState, service::keycloak::VerifiedPrincipal};

/// `dynamic_proxy_handler` is the main handler function that will try to get the route target config
/// and will try to prepare the request to forward/stream it to the target destination and will try to stream
/// back the response back to the original client that initiated the request in the first place
pub async fn dynamic_proxy_handler(
    State(state): State<AppState>,
    Extension(principal): Extension<VerifiedPrincipal>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    body: Body,
) -> Result<Response<Body>, StatusCode> {
    let client_id = principal.claims.effective_client_id().unwrap();
    let raw_path = uri.path();
    let query = uri.query().unwrap_or_default();

    debug!(
        method = %method,
        path = %raw_path,
        query = %query,
        client_id = %client_id,
        "Handling reverse proxy request"
    );

    // 1. Dynamic Route Resolution
    // Find the registered route prefix that matches the start of the incoming path
    let (prefix, target_config) = match state
        .service_routes
        .iter()
        .find(|entry| raw_path.starts_with(entry.key()))
        .map(|entry| (entry.key().clone(), entry.value().clone()))
    {
        Some(target) => target,
        None => {
            debug!(
                method = %method,
                path = %raw_path,
                "No reverse proxy route matched incoming path"
            );
            return Err(StatusCode::NOT_FOUND); // TODO: is it ok to reveal this state ??? 
        }
    };

    debug!(
        method = %method,
        path = %raw_path,
        matched_prefix = %prefix,
        upstream_base_url = %target_config.upstream_base_url,
        "Resolved reverse proxy route"
    );

    // 2. Authorization of the request
    if let Some(required_role) = target_config.required_role.as_deref() {
        let roles: Vec<String> = principal
            .claims
            .realm_access
            .roles
            .iter()
            .cloned()
            .chain(
                principal
                    .claims
                    .resource_access
                    .values()
                    .flat_map(|ra| ra.roles.iter().cloned()),
            )
            .collect();

        // if the caller/service does not have any of the required roles return forbidden
        if !roles.iter().any(|role| role == required_role) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    // 3. Path Rewriting
    // Example: Incoming "/api/v1/payments/invoices/123" with prefix "/api/v1/payments"
    // Strips prefix -> "/invoices/123"
    let sub_path = raw_path.strip_prefix(&prefix).unwrap_or("");

    // Preserve query parameters (e.g., "?status=paid")
    let query_string = uri.query().map(|q| format!("?{}", q)).unwrap_or_default();

    // Construct the new upstream URL
    let target_url_str = format!(
        "{}{}{}",
        target_config.upstream_base_url.trim_end_matches('/'),
        sub_path,
        query_string
    );

    debug!(
        method = %method,
        path = %raw_path,
        rewritten_sub_path = %sub_path,
        upstream_url = %target_url_str,
        "Rewrote path and built upstream URL"
    );

    let target_uri = reqwest::Url::parse(&target_url_str).map_err(|err| {
        warn!(
            method = %method,
            path = %raw_path,
            upstream_url = %target_url_str,
            error = %err,
            "Failed to parse upstream URL"
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // 4. Construct Forwarding Request
    let mut req_builder = state.http_client.request(method, target_uri);

    // Copy incoming headers (excluding host-specific headers)
    let mut forwarded_headers = 0usize;
    let mut skipped_headers = 0usize;
    for (name, value) in headers.iter() {
        if name != http::header::HOST {
            req_builder = req_builder.header(name, value);
            forwarded_headers += 1;
        } else {
            skipped_headers += 1;
        }
    }

    debug!(
        path = %raw_path,
        forwarded_headers,
        skipped_headers,
        "Prepared upstream request headers"
    );

    // Pass the request body stream to the upstream server
    let reqwest_body = reqwest::Body::wrap_stream(body.into_data_stream());
    let upstream_req = req_builder.body(reqwest_body);

    // 5. Send Request to Upstream Service
    let upstream_start = Instant::now();
    let upstream_resp = upstream_req.send().await.map_err(|err| {
        warn!(
            path = %raw_path,
            matched_prefix = %prefix,
            error = %err,
            "Upstream request failed"
        );
        StatusCode::BAD_GATEWAY
    })?;

    // 6. Convert Reqwest Response back to Axum Response
    let status = upstream_resp.status();
    let upstream_latency_ms = upstream_start.elapsed().as_millis();

    debug!(
        path = %raw_path,
        matched_prefix = %prefix,
        status = %status,
        upstream_latency_ms,
        response_headers = upstream_resp.headers().len(),
        "Received response from upstream service"
    );

    let mut response_builder = Response::builder().status(status);

    // Forward upstream response headers back to the original client
    for (name, value) in upstream_resp.headers().iter() {
        response_builder = response_builder.header(name, value);
    }

    // Stream response payload back to client without loading entire body into RAM
    let body_stream = upstream_resp.bytes_stream();
    let axum_body = Body::from_stream(body_stream);

    response_builder.body(axum_body).map_err(|err| {
        warn!(
            path = %raw_path,
            status = %status,
            error = %err,
            "Failed to build downstream response"
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })
}
