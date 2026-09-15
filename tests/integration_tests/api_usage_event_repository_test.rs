use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use pulse_gate::{
    core::config::common::ConfigManager,
    model::{
        api_usage_event::{ApiUsageEvent, UsageEventBus, UsageEventRepo, run_usage_event_worker},
        connection_postgres::build_postgres_pool,
    },
};
use tempfile::tempdir;

use crate::common::TestInfra;

async fn setup_usage_event_repo() -> Result<(UsageEventRepo, sqlx::PgPool)> {
    let config_dir_tmp = tempdir()?;
    let config_manager = ConfigManager::new(Some(config_dir_tmp.path().to_path_buf()));
    let app_config = config_manager.load_or_create()?.merge_with_env();
    let postgres_pool = build_postgres_pool(&app_config).await?;
    let repo = UsageEventRepo::new(postgres_pool.clone());

    repo.clear_all().await?;

    Ok((repo, postgres_pool))
}

fn usage_event(
    client_id: &str,
    target_id: &str,
    route_prefix: &str,
    method: &str,
    status: &str,
    response_code: Option<i32>,
    upstream_host: Option<&str>,
) -> ApiUsageEvent {
    ApiUsageEvent {
        client_id: client_id.to_string(),
        target_id: target_id.to_string(),
        route_prefix: route_prefix.to_string(),
        method: method.to_string(),
        status: status.to_string(),
        response_code,
        upstream_host: upstream_host.map(str::to_string),
        occurred_at: Utc::now(),
    }
}

#[tokio::test]
async fn insert_persists_new_usage_event() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, pool) = setup_usage_event_repo().await?;
    let expected = usage_event(
        "client-x",
        "target-a",
        "/api/v1/notifications",
        "GET",
        "success",
        Some(200),
        Some("http://target-a.internal"),
    );

    let saved = repo.insert(&expected).await?;

    assert_eq!(saved.client_id, expected.client_id);
    assert_eq!(saved.target_id, expected.target_id);
    assert_eq!(saved.route_prefix, expected.route_prefix);
    assert_eq!(saved.method, expected.method);
    assert_eq!(saved.status, expected.status);
    assert_eq!(saved.response_code, expected.response_code);
    assert_eq!(saved.upstream_host, expected.upstream_host);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_usage_event WHERE client_id = $1 AND target_id = $2",
    )
    .bind("client-x")
    .bind("target-a")
    .fetch_one(&pool)
    .await?;

    assert_eq!(count, 1);

    Ok(())
}

#[tokio::test]
async fn insert_tracks_multiple_events_for_same_client_and_target() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, pool) = setup_usage_event_repo().await?;

    repo.insert(&usage_event(
        "client-x",
        "target-a",
        "/api/v1/notifications",
        "GET",
        "success",
        Some(200),
        Some("http://target-a.internal"),
    ))
    .await?;
    repo.insert(&usage_event(
        "client-x",
        "target-a",
        "/api/v1/notifications",
        "POST",
        "accepted",
        Some(202),
        Some("http://target-a.internal"),
    ))
    .await?;
    repo.insert(&usage_event(
        "client-x",
        "target-a",
        "/api/v1/notifications",
        "GET",
        "rate_limited",
        Some(429),
        Some("http://target-a.internal"),
    ))
    .await?;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_usage_event WHERE client_id = $1 AND target_id = $2",
    )
    .bind("client-x")
    .bind("target-a")
    .fetch_one(&pool)
    .await?;

    assert_eq!(count, 3);

    Ok(())
}

#[tokio::test]
async fn insert_keeps_events_separate_per_client_and_target() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, pool) = setup_usage_event_repo().await?;

    repo.insert(&usage_event(
        "client-x",
        "target-a",
        "/api/v1/a",
        "GET",
        "success",
        Some(200),
        Some("http://target-a.internal"),
    ))
    .await?;
    repo.insert(&usage_event(
        "client-x",
        "target-b",
        "/api/v1/b",
        "GET",
        "success",
        Some(200),
        Some("http://target-b.internal"),
    ))
    .await?;
    repo.insert(&usage_event(
        "client-y",
        "target-a",
        "/api/v1/a",
        "POST",
        "accepted",
        Some(202),
        Some("http://target-a.internal"),
    ))
    .await?;

    let x_target_a: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_usage_event WHERE client_id = $1 AND target_id = $2",
    )
    .bind("client-x")
    .bind("target-a")
    .fetch_one(&pool)
    .await?;

    let x_target_b: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_usage_event WHERE client_id = $1 AND target_id = $2",
    )
    .bind("client-x")
    .bind("target-b")
    .fetch_one(&pool)
    .await?;

    let y_target_a: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_usage_event WHERE client_id = $1 AND target_id = $2",
    )
    .bind("client-y")
    .bind("target-a")
    .fetch_one(&pool)
    .await?;

    assert_eq!(x_target_a, 1);
    assert_eq!(x_target_b, 1);
    assert_eq!(y_target_a, 1);

    Ok(())
}

#[tokio::test]
async fn usage_event_bus_emits_events_to_the_worker_for_persistence() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, pool) = setup_usage_event_repo().await?;
    let (tx, rx) = tokio::sync::mpsc::channel::<ApiUsageEvent>(8);
    let bus = UsageEventBus::new(tx);

    let worker_repo = UsageEventRepo::new(pool.clone());
    let worker = tokio::spawn(run_usage_event_worker(rx, worker_repo));

    let event = usage_event(
        "client-z",
        "target-c",
        "/api/v1/reporting",
        "GET",
        "success",
        Some(200),
        Some("http://target-c.internal"),
    );

    bus.emit(event.clone()).await;
    tokio::time::sleep(Duration::from_millis(250)).await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_usage_event WHERE client_id = $1 AND target_id = $2",
    )
    .bind("client-z")
    .bind("target-c")
    .fetch_one(&pool)
    .await?;

    assert_eq!(count, 1);

    worker.abort();
    let _ = worker.await;

    let persisted = repo
        .insert(&usage_event(
            "client-z",
            "target-c",
            "/api/v1/reporting",
            "POST",
            "accepted",
            Some(202),
            Some("http://target-c.internal"),
        ))
        .await?;
    assert_eq!(persisted.client_id, "client-z");
    assert_eq!(persisted.target_id, "target-c");

    Ok(())
}

#[tokio::test]
async fn clear_all_removes_all_usage_events_from_the_repository() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, pool) = setup_usage_event_repo().await?;

    repo.insert(&usage_event(
        "client-a",
        "target-a",
        "/api/v1/alpha",
        "GET",
        "success",
        Some(200),
        Some("http://target-a.internal"),
    ))
    .await?;
    repo.insert(&usage_event(
        "client-b",
        "target-b",
        "/api/v1/beta",
        "POST",
        "accepted",
        Some(202),
        Some("http://target-b.internal"),
    ))
    .await?;

    let row_count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_usage_event")
        .fetch_one(&pool)
        .await?;
    assert_eq!(row_count_before, 2);

    repo.clear_all().await?;

    let row_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_usage_event")
        .fetch_one(&pool)
        .await?;
    assert_eq!(row_count_after, 0);

    Ok(())
}

#[tokio::test]
async fn clear_all_is_idempotent_when_repository_is_already_empty() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, pool) = setup_usage_event_repo().await?;

    repo.clear_all().await?;
    repo.clear_all().await?;

    let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_usage_event")
        .fetch_one(&pool)
        .await?;
    assert_eq!(row_count, 0);

    Ok(())
}
