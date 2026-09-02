use anyhow::Result;
use serde_json::{Value, json};
use tempfile::tempdir;

use crate::common::{TestInfra, default_app_router};

#[tokio::test]
async fn test_cluster_ping_is_successful() -> Result<()> {
    // ask for access to shared testing infrastructure
    let test_infra = TestInfra::get_infra().await;
    // lock the access
    let _lock = test_infra.lock.lock().await;
    // export env variables for the testing process, env_restore will restore the previous state
    let _env_restore = test_infra.export_env_variables(None);
    // spawn the app process (default router)
    let config_dir_tmp = tempdir().expect("failed to create temp dir");
    let test_server = test_infra
        .spawn_app(default_app_router(&config_dir_tmp).await?)
        .await;
    let app_url = test_server.app_url();

    // exercise - execute real HTTP calls against the bound Axum instance
    let res = test_infra
        .http_client
        .get(format!("{app_url}/health"))
        .send()
        .await
        .unwrap();

    // verify
    let status = res.status();
    let body = res.text().await?;

    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&body)?,
        json!({
            "status": "ok",
            "service": "PulseGate",
            "redis": "ok",
            "postgres": "ok",
            "keycloak": "ok",
            "errors": {
                "redis": null,
                "postgres": null,
                "keycloak": null,
            }
        })
    );

    Ok(())
}
