use std::{sync::Arc, time::Duration};

use anyhow::Result;
use dashmap::DashMap;
use pulse_gate::{
    core::config::common::ConfigManager,
    model::{
        connection_postgres::build_postgres_pool,
        connection_redis::build_redis_pool,
        route_target::{RouteTarget, RouteTargetRepo},
        route_target_event::{RouteTargetEvent, RouteTargetEventRepo},
    },
};
use tempfile::tempdir;

use crate::common::TestInfra;

async fn setup_route_target_repo() -> Result<(RouteTargetRepo, sqlx::PgPool)> {
    let config_dir_tmp = tempdir()?;
    let config_manager = ConfigManager::new(Some(config_dir_tmp.path().to_path_buf()));
    let app_config = config_manager.load_or_create()?.merge_with_env();
    let postgres_pool = build_postgres_pool(&app_config).await?;
    let route_target_repo = RouteTargetRepo::new(postgres_pool.clone());
    route_target_repo.clear_all().await?;

    Ok((route_target_repo, postgres_pool))
}

fn route_target(
    path_prefix: &str,
    upstream_base_url: &str,
    rate_limit_per_min: i32,
    required_role: Option<&str>,
) -> RouteTarget {
    RouteTarget::new(
        path_prefix.to_string(),
        upstream_base_url.to_string(),
        rate_limit_per_min,
        required_role.map(str::to_string),
    )
}

#[tokio::test]
async fn create_route_target_persists_the_new_route() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;
    let expected = route_target(
        "/checkout",
        "http://checkout.internal",
        120,
        Some("checkout-admin"),
    );

    let created = repo.create_route_target(expected.clone()).await?;

    assert_eq!(created, expected);
    assert_eq!(
        repo.get_route_target_by_path_prefix("/checkout").await?,
        expected
    );

    Ok(())
}

#[tokio::test]
async fn create_route_target_fails_for_duplicate_path_prefix() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;
    let route_target = route_target("/orders", "http://orders.internal", 90, None);

    repo.create_route_target(route_target.clone()).await?;

    let duplicate_result = repo.create_route_target(route_target).await;

    assert!(duplicate_result.is_err());

    Ok(())
}

#[tokio::test]
async fn get_route_target_by_path_prefix_returns_the_matching_route() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;
    let expected = route_target(
        "/inventory",
        "http://inventory.internal",
        45,
        Some("inventory-read"),
    );

    repo.create_route_target(expected.clone()).await?;

    let actual = repo.get_route_target_by_path_prefix("/inventory").await?;

    assert_eq!(actual, expected);

    Ok(())
}

#[tokio::test]
async fn get_route_target_by_path_prefix_fails_for_missing_route() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;

    let result = repo
        .get_route_target_by_path_prefix("/does-not-exist")
        .await;

    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
async fn list_route_targets_returns_all_entries_sorted_by_path_prefix() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;
    let gamma = route_target("/gamma", "http://gamma.internal", 15, Some("gamma-role"));
    let alpha = route_target("/alpha", "http://alpha.internal", 60, None);
    let beta = route_target("/beta", "http://beta.internal", 22, Some("beta-role"));

    repo.create_route_target(gamma.clone()).await?;
    repo.create_route_target(alpha.clone()).await?;
    repo.create_route_target(beta.clone()).await?;

    let routes = repo.list_route_targets().await?;
    let path_prefixes = routes
        .iter()
        .map(|route| route.path_prefix.clone())
        .collect::<Vec<_>>();

    assert_eq!(path_prefixes, vec!["/alpha", "/beta", "/gamma"]);
    assert!(routes.iter().any(|route| route == &alpha));
    assert!(routes.iter().any(|route| route == &beta));
    assert!(routes.iter().any(|route| route == &gamma));

    Ok(())
}

#[tokio::test]
async fn update_route_target_updates_existing_route() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;
    let original = route_target(
        "/billing",
        "http://billing.internal",
        30,
        Some("billing-view"),
    );
    let updated = route_target(
        "/billing",
        "http://billing-updated.internal",
        75,
        Some("billing-admin"),
    );

    repo.create_route_target(original).await?;

    let actual = repo.update_route_target(updated.clone()).await?;

    assert_eq!(actual, updated);
    assert_eq!(
        repo.get_route_target_by_path_prefix("/billing").await?,
        updated
    );

    Ok(())
}

#[tokio::test]
async fn update_route_target_fails_for_missing_route() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;
    let missing = route_target("/missing", "http://missing.internal", 12, None);

    let result = repo.update_route_target(missing).await;

    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
async fn delete_route_target_by_path_prefix_removes_the_route() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;
    let route = route_target(
        "/archive",
        "http://archive.internal",
        40,
        Some("archive-admin"),
    );

    repo.create_route_target(route).await?;

    repo.delete_route_target_by_path_prefix("/archive").await?;

    let result = repo.get_route_target_by_path_prefix("/archive").await;
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
async fn delete_route_target_by_path_prefix_fails_for_missing_route() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;

    let result = repo
        .delete_route_target_by_path_prefix("/missing-for-delete")
        .await;

    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
async fn clear_all_removes_all_route_targets_from_the_repository() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;

    repo.create_route_target(route_target(
        "/first",
        "http://first.internal",
        10,
        Some("first-role"),
    ))
    .await?;
    repo.create_route_target(route_target(
        "/second",
        "http://second.internal",
        20,
        Some("second-role"),
    ))
    .await?;

    repo.clear_all().await?;

    let routes = repo.list_route_targets().await?;
    assert!(routes.is_empty());

    Ok(())
}

#[tokio::test]
async fn clear_all_is_idempotent_when_repository_is_already_empty() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let (repo, _) = setup_route_target_repo().await?;

    repo.clear_all().await?;

    let routes = repo.list_route_targets().await?;
    assert!(routes.is_empty());

    Ok(())
}

#[tokio::test]
async fn hydrate_route_versions_seeds_the_redis_route_version_hash() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let config_dir_tmp = tempdir()?;
    let config_manager = ConfigManager::new(Some(config_dir_tmp.path().to_path_buf()));
    let app_config = config_manager.load_or_create()?.merge_with_env();
    let redis_pool = build_redis_pool(&app_config).await?;
    let event_repo = RouteTargetEventRepo::new(redis_pool.clone());

    let service_routes: Arc<DashMap<String, RouteTarget>> = Arc::new(DashMap::new());
    let route_versions: Arc<DashMap<String, u64>> = Arc::new(DashMap::new());
    let route = route_target(
        "/seeded-route",
        "http://seeded-route.internal",
        42,
        Some("seeded-role"),
    );

    service_routes.insert(route.path_prefix.clone(), route.clone());
    event_repo
        .hydrate_route_versions(&service_routes, &route_versions)
        .await?;

    assert_eq!(
        route_versions.get(&route.path_prefix).map(|value| *value),
        Some(0)
    );
    assert_eq!(
        event_repo.current_route_version(&route.path_prefix).await?,
        0
    );

    Ok(())
}

#[tokio::test]
async fn publish_route_event_updates_local_cache_via_listener() -> Result<()> {
    let test_infra = TestInfra::get_infra().await;
    let _lock = test_infra.lock.lock().await;
    let _env_restore = test_infra.export_env_variables(None);

    let config_dir_tmp = tempdir()?;
    let config_manager = ConfigManager::new(Some(config_dir_tmp.path().to_path_buf()));
    let app_config = config_manager.load_or_create()?.merge_with_env();
    let redis_pool = build_redis_pool(&app_config).await?;
    let event_repo = RouteTargetEventRepo::new(redis_pool.clone());

    let service_routes: Arc<DashMap<String, RouteTarget>> = Arc::new(DashMap::new());
    let route_versions: Arc<DashMap<String, u64>> = Arc::new(DashMap::new());
    let listener_routes = service_routes.clone();
    let listener_versions = route_versions.clone();
    event_repo
        .start_route_change_listener(
            listener_routes,
            listener_versions,
            &app_config.redis_config.url,
        )
        .await?;

    let route = route_target(
        "/route-sync",
        "http://route-sync.internal",
        42,
        Some("route-sync-role"),
    );
    let newer_route = route_target(
        "/route-sync",
        "http://route-sync-new.internal",
        84,
        Some("route-sync-role-updated"),
    );

    let stale_version = 1_u64;
    let current_version = 2_u64;

    event_repo
        .publish_route_event(&RouteTargetEvent::updated(
            newer_route.clone(),
            current_version,
        ))
        .await?;
    event_repo
        .publish_route_event(&RouteTargetEvent::updated(route.clone(), stale_version))
        .await?;

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if service_routes.contains_key(&route.path_prefix) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;

    let cached = service_routes.get(&route.path_prefix).unwrap();
    assert_eq!(*cached, newer_route);
    assert_eq!(
        *route_versions.get(&route.path_prefix).unwrap(),
        current_version
    );

    Ok(())
}
