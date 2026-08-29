use anyhow::{Context, Result};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};
use std::time::Duration;
use tokio::time::sleep;

use crate::core::config::common::AppConfig;

const POSTGRES_STARTUP_RETRIES: usize = 3;
const POSTGRES_STARTUP_RETRY_DELAY: Duration = Duration::from_millis(500);

/// `build_postgres_pool` creates a pool of postgres connections
pub async fn build_postgres_pool(app_config: &AppConfig) -> Result<PgPool> {
    let mut options: PgConnectOptions = app_config
        .postgres_config
        .url
        .parse()
        .context("failed to parse postgres connection url")?;

    let ssl_mode = app_config
        .postgres_config
        .ssl_mode
        .parse::<PgSslMode>()
        .context("invalid postgres ssl mode")?;
    options = options.ssl_mode(ssl_mode);

    if let Some(timeout_ms) = app_config.postgres_config.statement_timeout_ms {
        options = options.options([("statement_timeout", format!("{}ms", timeout_ms))]);
    }

    let pg_pool = PgPoolOptions::new()
        .max_connections(app_config.postgres_config.max_connections)
        .min_connections(app_config.postgres_config.min_connections)
        .acquire_timeout(Duration::from_millis(
            app_config.postgres_config.acquire_timeout_ms,
        ))
        .idle_timeout(Some(Duration::from_secs(
            app_config.postgres_config.idle_timeout_secs,
        )))
        .max_lifetime(Some(Duration::from_secs(
            app_config.postgres_config.max_lifetime_secs,
        )))
        .connect_with(options)
        .await
        .context("failed to create postgres connection pool")?;

    if app_config.postgres_config.validate_on_startup {
        validate_postgres_startup(&pg_pool).await?;
    }

    Ok(pg_pool)
}

/// `validate_postgres_startup` checks the postgres pool by acquiring a connection and running a simple query.
pub async fn validate_postgres_startup(pg_pool: &PgPool) -> Result<()> {
    let mut attempt = 0;

    loop {
        attempt += 1;

        let mut conn = match pg_pool.acquire().await {
            Ok(conn) => conn,
            Err(err) => {
                if attempt >= POSTGRES_STARTUP_RETRIES {
                    return Err(err).context(
                        "failed to acquire postgres connection during startup validation",
                    );
                }
                tracing::warn!(
                    attempt,
                    "postgres startup validation acquire failed, retrying"
                );
                sleep(POSTGRES_STARTUP_RETRY_DELAY).await;
                continue;
            }
        };

        let result = sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&mut *conn)
            .await;

        match result {
            Ok(1) => return Ok(()),
            Ok(value) => {
                let err =
                    anyhow::anyhow!("unexpected postgres startup validation result: {}", value);
                return Err(err).context("postgres startup validation failed");
            }
            Err(err) if attempt < POSTGRES_STARTUP_RETRIES => {
                tracing::warn!(
                    attempt,
                    "postgres startup validation query failed, retrying: {err}"
                );
                sleep(POSTGRES_STARTUP_RETRY_DELAY).await;
            }
            Err(err) => return Err(err).context("postgres startup validation failed"),
        }
    }
}
