use anyhow::Result;
use pulse_gate::{
    core::config::common::ConfigManager, model::connection_postgres::build_postgres_pool,
};
use reqwest::StatusCode;
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

fn route_payload(
    path_prefix: &str,
    upstream_base_url: &str,
    rate_limit_per_min: i32,
    required_role: Option<&str>,
) -> Value {
    json!({
        "path_prefix": path_prefix,
        "upstream_base_url": upstream_base_url,
        "rate_limit_per_min": rate_limit_per_min,
        "required_role": required_role,
    })
}

#[tokio::test]
async fn manage_list_route_targets_returns_all_routes() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();

    let create_1 = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .json(&route_payload(
            "/alpha",
            "http://alpha.internal",
            12,
            Some("alpha-role"),
        ))
        .send()
        .await?;
    assert_eq!(create_1.status(), StatusCode::CREATED);

    let create_2 = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .json(&route_payload(
            "/beta",
            "http://beta.internal",
            20,
            Some("beta-role"),
        ))
        .send()
        .await?;
    assert_eq!(create_2.status(), StatusCode::CREATED);

    let list = test_infra
        .http_client
        .get(format!("{app_url}/manage/route-targets"))
        .send()
        .await?;

    assert_eq!(list.status(), StatusCode::OK);
    let body: Vec<Value> = list.json().await?;
    assert_eq!(body.len(), 2);
    assert!(body.iter().any(|item| item["path_prefix"] == "/alpha"));
    assert!(body.iter().any(|item| item["path_prefix"] == "/beta"));

    Ok(())
}

#[tokio::test]
async fn manage_create_route_target_persists_and_returns_created_record() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();

    let response = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .json(&route_payload(
            "/checkout",
            "http://checkout.internal",
            80,
            Some("checkout-admin"),
        ))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::CREATED);
    let body: Value = response.json().await?;
    assert_eq!(body["path_prefix"], "/checkout");
    assert_eq!(body["upstream_base_url"], "http://checkout.internal");
    assert_eq!(body["rate_limit_per_min"], 80);

    let fetched = test_infra
        .http_client
        .get(format!("{app_url}/manage/route-targets/checkout"))
        .send()
        .await?;

    assert_eq!(fetched.status(), StatusCode::OK);
    let fetched_body: Value = fetched.json().await?;
    assert_eq!(fetched_body["path_prefix"], "/checkout");

    Ok(())
}

#[tokio::test]
async fn manage_create_route_target_rejects_malformed_path_prefix() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();

    let response = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .json(&route_payload(
            "bad-prefix",
            "http://good.internal",
            5,
            None,
        ))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    Ok(())
}

#[tokio::test]
async fn manage_create_route_target_rejects_empty_upstream_url() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();

    let response = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .json(&route_payload("/orders", "", 10, None))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    Ok(())
}

#[tokio::test]
async fn manage_update_route_target_updates_existing_record() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();

    let create = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .json(&route_payload(
            "/billing",
            "http://billing.internal",
            30,
            Some("billing-read"),
        ))
        .send()
        .await?;
    assert_eq!(create.status(), StatusCode::CREATED);

    let update = test_infra
        .http_client
        .put(format!("{app_url}/manage/route-targets/billing"))
        .json(&route_payload(
            "/billing",
            "http://billing-updated.internal",
            55,
            Some("billing-admin"),
        ))
        .send()
        .await?;

    assert_eq!(update.status(), StatusCode::OK);
    let body: Value = update.json().await?;
    assert_eq!(body["upstream_base_url"], "http://billing-updated.internal");
    assert_eq!(body["rate_limit_per_min"], 55);

    Ok(())
}

#[tokio::test]
async fn manage_delete_route_target_removes_existing_record() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = reset_route_targets(&test_infra).await?;

    let config_dir_tmp = tempdir()?;
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();

    let create = test_infra
        .http_client
        .post(format!("{app_url}/manage/route-targets"))
        .json(&route_payload(
            "/archive",
            "http://archive.internal",
            14,
            Some("archive-role"),
        ))
        .send()
        .await?;
    assert_eq!(create.status(), StatusCode::CREATED);

    let delete = test_infra
        .http_client
        .delete(format!("{app_url}/manage/route-targets/archive"))
        .send()
        .await?;
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    let fetch = test_infra
        .http_client
        .get(format!("{app_url}/manage/route-targets/archive"))
        .send()
        .await?;
    assert_eq!(fetch.status(), StatusCode::NOT_FOUND);

    Ok(())
}
