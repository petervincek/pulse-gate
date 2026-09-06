use anyhow::Result;
use pulse_gate::{
    core::config::common::ConfigManager,
    model::{call_stats::CallStatsRepo, connection_redis::build_redis_pool},
};
use tempfile::tempdir;

use crate::common::TestInfra;

async fn setup_call_stats_repo() -> Result<CallStatsRepo> {
    let config_dir_tmp = tempdir()?;
    let config_manager = ConfigManager::new(Some(config_dir_tmp.path().to_path_buf()));
    let app_config = config_manager.load_or_create()?.merge_with_env();
    let redis_pool = build_redis_pool(&app_config).await?;
    let repo = CallStatsRepo::new(redis_pool);
    repo.clear_all().await?;
    Ok(repo)
}

async fn total_calls_for_client(
    repo: &CallStatsRepo,
    target_id: &str,
    client_id: &str,
) -> Result<i64> {
    repo.get_total_calls_for_client(target_id, client_id).await
}

async fn current_minute_calls_for_client(
    repo: &CallStatsRepo,
    target_id: &str,
    client_id: &str,
) -> Result<i64> {
    repo.get_current_minute_calls_for_client(target_id, client_id)
        .await
}

#[tokio::test]
async fn record_call_increments_total_call_count_for_target_and_client() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    repo.record_call("target-a", "client-x").await?;
    repo.record_call("target-a", "client-x").await?;
    repo.record_call("target-a", "client-x").await?;

    let total = total_calls_for_client(&repo, "target-a", "client-x").await?;
    assert_eq!(total, 3);

    Ok(())
}

#[tokio::test]
async fn record_call_tracks_counts_per_client_separately_for_same_target() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    repo.record_call("target-a", "client-x").await?;
    repo.record_call("target-a", "client-x").await?;
    repo.record_call("target-a", "client-y").await?;

    let x_total = total_calls_for_client(&repo, "target-a", "client-x").await?;
    let y_total = total_calls_for_client(&repo, "target-a", "client-y").await?;

    assert_eq!(x_total, 2);
    assert_eq!(y_total, 1);

    Ok(())
}

#[tokio::test]
async fn record_call_tracks_counts_per_target_separately_for_same_client() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    repo.record_call("target-a", "client-x").await?;
    repo.record_call("target-b", "client-x").await?;
    repo.record_call("target-b", "client-x").await?;

    let a_total = total_calls_for_client(&repo, "target-a", "client-x").await?;
    let b_total = total_calls_for_client(&repo, "target-b", "client-x").await?;

    assert_eq!(a_total, 1);
    assert_eq!(b_total, 2);

    Ok(())
}

#[tokio::test]
async fn check_minute_limit_allows_requests_within_the_limit() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    let first = repo.check_minute_limit("target-a", "client-x", 3).await?;
    let second = repo.check_minute_limit("target-a", "client-x", 3).await?;

    assert!(first.allowed);
    assert_eq!(first.current_count, 1);
    assert!(second.allowed);
    assert_eq!(second.current_count, 2);

    Ok(())
}

#[tokio::test]
async fn check_minute_limit_rejects_requests_that_exceed_the_limit() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    let first = repo.check_minute_limit("target-a", "client-x", 1).await?;
    let second = repo.check_minute_limit("target-a", "client-x", 1).await?;
    let third = repo.check_minute_limit("target-a", "client-x", 1).await?;

    assert!(first.allowed);
    assert_eq!(first.current_count, 1);
    assert!(!second.allowed);
    assert_eq!(second.current_count, 2);
    assert!(!third.allowed);
    assert_eq!(third.current_count, 3);

    Ok(())
}

#[tokio::test]
async fn rate_limit_is_scoped_per_client_and_target() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    let x_first = repo.check_minute_limit("target-a", "client-x", 1).await?;
    let y_first = repo.check_minute_limit("target-a", "client-y", 1).await?;
    let x_second = repo.check_minute_limit("target-a", "client-x", 1).await?;

    assert!(x_first.allowed);
    assert!(y_first.allowed);
    assert!(!x_second.allowed);

    Ok(())
}

#[tokio::test]
async fn record_call_and_check_minute_limit_tracks_total_and_enforces_limit_together() -> Result<()>
{
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    let first = repo
        .record_call_and_check_minute_limit("target-a", "client-x", 1)
        .await?;
    let second = repo
        .record_call_and_check_minute_limit("target-a", "client-x", 1)
        .await?;

    assert!(first.allowed);
    assert_eq!(first.current_count, 1);
    assert!(!second.allowed);
    assert_eq!(second.current_count, 2);

    let total = total_calls_for_client(&repo, "target-a", "client-x").await?;
    assert_eq!(total, 2);

    Ok(())
}

#[tokio::test]
async fn get_usage_snapshot_for_client_returns_total_and_current_minute_usage() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    repo.record_call("target-a", "client-x").await?;
    repo.record_call("target-a", "client-x").await?;
    repo.check_minute_limit("target-a", "client-x", 10).await?;
    repo.check_minute_limit("target-a", "client-x", 10).await?;

    let snapshot = repo
        .get_usage_snapshot_for_client("target-a", "client-x")
        .await?;

    assert_eq!(snapshot.target_id, "target-a");
    assert_eq!(snapshot.client_id, "client-x");
    assert_eq!(snapshot.total_calls, 2);
    assert_eq!(snapshot.calls_in_current_minute, 2);
    assert!(!snapshot.current_minute_bucket.is_empty());

    Ok(())
}

#[tokio::test]
async fn clear_all_removes_all_call_stats_and_rate_limit_keys() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    repo.record_call("target-a", "client-x").await?;
    repo.check_minute_limit("target-a", "client-x", 10).await?;

    assert_ne!(
        total_calls_for_client(&repo, "target-a", "client-x").await?,
        0
    );
    assert_ne!(
        current_minute_calls_for_client(&repo, "target-a", "client-x").await?,
        0
    );

    repo.clear_all().await?;

    assert_eq!(
        total_calls_for_client(&repo, "target-a", "client-x").await?,
        0
    );
    assert_eq!(
        current_minute_calls_for_client(&repo, "target-a", "client-x").await?,
        0
    );

    Ok(())
}

#[tokio::test]
async fn clear_all_is_idempotent_when_repository_is_already_empty() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let repo = setup_call_stats_repo().await?;

    repo.clear_all().await?;
    repo.clear_all().await?;

    assert_eq!(
        total_calls_for_client(&repo, "target-a", "client-x").await?,
        0
    );
    assert_eq!(
        current_minute_calls_for_client(&repo, "target-a", "client-x").await?,
        0
    );

    Ok(())
}
