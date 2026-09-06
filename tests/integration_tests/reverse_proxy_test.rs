use anyhow::Result;
use pulse_gate::{
    core::config::common::ConfigManager, model::connection_postgres::build_postgres_pool,
};
use reqwest::{StatusCode, header::AUTHORIZATION};
use serde_json::{Value, json};
use tempfile::tempdir;

use crate::common::{EnvRestore, TestInfra, default_app_router};

async fn reset_route_targets(test_infra: &TestInfra) -> Result<EnvRestore> {
    let env_restore = test_infra.export_env_variables(None);

    let config_dir_tmp = tempdir()?;
    let app_config = ConfigManager::new(Some(config_dir_tmp.path().to_path_buf()))
        .load_or_create()?
        .merge_with_env();
    let pg_pool = build_postgres_pool(&app_config).await?;

    sqlx::query("TRUNCATE TABLE route_target")
        .execute(&pg_pool)
        .await?;

    Ok(env_restore)
}

async fn admin_auth_header(test_infra: &TestInfra) -> Result<String> {
    let token = test_infra
        .get_client_credentials_token("pulse-gate-admin", "pulse-gate-admin-secret")
        .await?;
    Ok(format!("Bearer {token}"))
}

async fn target_service_auth_header(
    test_infra: &TestInfra,
    client_id: &str,
    client_secret: &str,
) -> Result<String> {
    let token = test_infra
        .get_client_credentials_token(client_id, client_secret)
        .await?;
    Ok(format!("Bearer {token}"))
}

fn proxy_route_payload(upstream_base_url: &str, required_role: &str) -> Value {
    json!({
        "path_prefix": "/api/v1/target-service-a",
        "upstream_base_url": upstream_base_url,
        "rate_limit_per_min": 60,
        "required_role": required_role,
    })
}

#[tokio::test]
async fn reverse_proxy_allows_valid_service_account_with_required_role() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;

    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();
    let admin_header = admin_auth_header(&test_infra).await?;

    let create_route = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .header(AUTHORIZATION, &admin_header)
        .json(&proxy_route_payload(
            &test_infra.target_service_a_url(),
            "target-service-a",
        ))
        .send()
        .await?;

    assert_eq!(
        create_route.status(),
        StatusCode::CREATED,
        "{:?}",
        create_route.text().await?
    );

    let target_service_header =
        target_service_auth_header(&test_infra, "client-x-service", "client-x-service-secret")
            .await?;
    let response = test_infra
        .http_client
        .get(format!("{app_url}/api/v1/target-service-a/payment"))
        .header(AUTHORIZATION, &target_service_header)
        .send()
        .await?;

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        response.text().await?
    );

    Ok(())
}

#[tokio::test]
async fn reverse_proxy_rejects_request_when_required_role_is_missing() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;

    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();
    let admin_header = admin_auth_header(&test_infra).await?;

    let create_route = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .header(AUTHORIZATION, &admin_header)
        .json(&proxy_route_payload(
            &test_infra.target_service_a_url(),
            "target-service-a",
        ))
        .send()
        .await?;

    assert_eq!(
        create_route.status(),
        StatusCode::CREATED,
        "{:?}",
        create_route.text().await?
    );

    let response = test_infra
        .http_client
        .get(format!("{app_url}/api/v1/target-service-a/payment"))
        .header(AUTHORIZATION, &admin_header)
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    Ok(())
}

#[tokio::test]
async fn reverse_proxy_rejects_missing_authorization_header() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;

    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();
    let admin_header = admin_auth_header(&test_infra).await?;

    let create_route = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .header(AUTHORIZATION, &admin_header)
        .json(&proxy_route_payload(
            &test_infra.target_service_a_url(),
            "target-service-a",
        ))
        .send()
        .await?;

    assert_eq!(
        create_route.status(),
        StatusCode::CREATED,
        "{:?}",
        create_route.text().await?
    );

    let response = test_infra
        .http_client
        .get(format!("{app_url}/api/v1/target-service-a/payment"))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    Ok(())
}

#[tokio::test]
async fn reverse_proxy_rejects_expired_or_invalid_bearer_token() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;

    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();
    let admin_header = admin_auth_header(&test_infra).await?;

    let create_route = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .header(AUTHORIZATION, &admin_header)
        .json(&proxy_route_payload(
            &test_infra.target_service_a_url(),
            "target-service-a",
        ))
        .send()
        .await?;

    assert_eq!(
        create_route.status(),
        StatusCode::CREATED,
        "{:?}",
        create_route.text().await?
    );

    let expired_or_invalid_token = "Bearer eyJhbGciOiJSUzI1NiIsImtpZCI6ImV4cGlyZWQtdG9rZW4ifQ.eyJzdWIiOiJ0ZXN0LXVzZXIiLCJpc3MiOiJodHRwOi8vbG9jYWxob3N0OjgwODAvcmVhbG1zL2dhdGV3YXktcmVhbG0iLCJleHAiOjE3MDAwMDAwMDB9.signature";
    let response = test_infra
        .http_client
        .get(format!("{app_url}/api/v1/target-service-a/payment"))
        .header(AUTHORIZATION, expired_or_invalid_token)
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    Ok(())
}
