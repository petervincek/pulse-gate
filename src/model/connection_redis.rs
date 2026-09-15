use anyhow::{Context, Result};
use deadpool_redis::{Config as DeadpoolConfig, Pool as RedisPool, Runtime as RedisRuntime};
use redis::AsyncCommands;
use std::time::Duration;
use tokio::time::sleep;

use crate::core::config::common::AppConfig;

const REDIS_STARTUP_RETRIES: usize = 3;
const REDIS_STARTUP_RETRY_DELAY: Duration = Duration::from_millis(500);

/// `build_redis_pool` creates a pool of redis connections
pub async fn build_redis_pool(app_config: &AppConfig) -> Result<RedisPool> {
    let mut cfg = DeadpoolConfig::from_url(app_config.redis_config.url.clone());

    // adust the pool configuration
    let pool_cfg = cfg.pool.get_or_insert_with(Default::default);
    pool_cfg.max_size = app_config.redis_config.pool_max_size;
    pool_cfg.timeouts.create = Some(Duration::from_millis(
        app_config.redis_config.pool_timeout_ms,
    ));
    pool_cfg.timeouts.wait = Some(Duration::from_millis(
        app_config.redis_config.pool_wait_timeout_ms,
    ));
    pool_cfg.timeouts.recycle = Some(Duration::from_secs(
        app_config.redis_config.pool_recycle_seconds,
    ));

    // create the actual pool of redis connections
    let pool = cfg.create_pool(Some(RedisRuntime::Tokio1))?;

    if app_config.redis_config.validate_on_startup {
        validate_redis_startup(&pool).await?;
    }

    Ok(pool)
}

/// `validate_redis_startup` checks the redis pool
async fn validate_redis_startup(pool: &RedisPool) -> Result<()> {
    let mut attempt = 0;

    loop {
        attempt += 1;

        // try to get the connection from the pool
        let mut conn = match pool.get().await {
            Ok(conn) => conn,
            Err(err) => {
                if attempt >= REDIS_STARTUP_RETRIES {
                    return Err(err)
                        .context("failed to get redis connection during startup validation");
                }
                tracing::warn!(
                    attempt,
                    "redis startup validation connection failed, retrying"
                );
                sleep(REDIS_STARTUP_RETRY_DELAY).await;
                continue;
            }
        };

        match conn.ping::<String>().await {
            Ok(_) => return Ok(()),
            Err(err) if attempt < REDIS_STARTUP_RETRIES => {
                tracing::warn!(
                    attempt,
                    "redis startup validation ping failed, retrying: {err}"
                );
                sleep(REDIS_STARTUP_RETRY_DELAY).await;
            }
            Err(err) => return Err(err).context("redis startup validation failed"),
        }
    }
}
