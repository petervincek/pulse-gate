use anyhow::{Context, Result};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use tracing::debug;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitDecision {
    pub allowed: bool,
    pub current_count: u64,
    pub limit_per_minute: u64,
    pub bucket: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetClientUsageSnapshot {
    pub target_id: String,
    pub client_id: String,
    pub total_calls: i64,
    pub calls_in_current_minute: i64,
    pub current_minute_bucket: String,
}

/// `CallStatsRepo` encapsulates Redis-based usage and rate-limit tracking.
///
/// The design intentionally separates the concerns:
/// - a total counter per target/client for analytics and billing
/// - a per-minute counter per target/client for enforcement
///
/// This makes it safe across multiple gateway instances behind a load balancer because
/// all counters are stored in Redis and updated atomically using Redis commands.
#[derive(Debug, Clone)]
pub struct CallStatsRepo {
    redis_pool: RedisPool,
}

impl CallStatsRepo {
    pub fn new(redis_pool: RedisPool) -> Self {
        Self { redis_pool }
    }

    /// `record_call` records the call to the target service by client that holds that client id
    pub async fn record_call(&self, target_id: &str, client_id: &str) -> Result<()> {
        let mut conn = self
            .redis_pool
            .get()
            .await
            .context("failed to acquire redis connection for call tracking")?;

        // data structure to store the total count for target service is Redis Hash
        // where the key of that hash is client id and value is the number of total calls that is incremented
        let total_key = format!("pulse-gate:stats:target:{}:clients", target_id);
        let count: i64 = redis::cmd("HINCRBY") // increase by one Redis Hash key - client id
            .arg(&[&total_key, client_id, "1"])
            .query_async(&mut conn)
            .await
            .context("failed to increment target/client call total")?;

        debug!(
            target_id,
            client_id,
            total_count = count,
            "Recorded call for target/client in Redis"
        );

        Ok(())
    }

    /// `check_minute_limit` is responsible for enforcing the rate limiting for a caller (client id) to target service
    /// based on provided rate limit
    pub async fn check_minute_limit(
        &self,
        target_id: &str,
        client_id: &str,
        limit_per_minute: i32,
    ) -> Result<RateLimitDecision> {
        // create a identifier of a specific minute window
        let bucket = Utc::now().format("%Y%m%d%H%M").to_string();
        // keep the track in data structure that spefic to the target, client and minute window
        let rate_key = format!(
            "pulse-gate:rate:target:{}:client:{}:minute:{}",
            target_id, client_id, bucket
        );

        let mut conn = self
            .redis_pool
            .get()
            .await
            .context("failed to acquire redis connection for rate limit check")?;

        // Execute INCR and EXPIRE atomically in a single pipeline round-trip
        // get the current count and auto-cleanup through expiration
        let (current_count, _): (i64, bool) = redis::pipe()
            .atomic() // Encloses operations inside a MULTI/EXEC transaction block
            .incr(&rate_key, 1)
            .expire(&rate_key, 70)
            .query_async(&mut conn)
            .await
            .context("failed to execute rate limit pipeline")?;

        let limit_per_minute = u64::try_from(limit_per_minute.max(0)).unwrap_or_default();
        // enforce the rate limiting policy
        let allowed = limit_per_minute == 0 || (current_count as u64) <= limit_per_minute;

        tracing::debug!(
            target_id,
            client_id,
            bucket,
            current_count,
            limit_per_minute,
            allowed,
            "Checked current-minute rate limit"
        );

        // share the result
        Ok(RateLimitDecision {
            allowed,
            current_count: current_count as u64,
            limit_per_minute,
            bucket,
        })
    }

    pub async fn record_call_and_check_minute_limit(
        &self,
        target_id: &str,
        client_id: &str,
        limit_per_minute: i32,
    ) -> Result<RateLimitDecision> {
        self.record_call(target_id, client_id).await?;
        self.check_minute_limit(target_id, client_id, limit_per_minute)
            .await
    }

    pub async fn get_total_calls_for_client(
        &self,
        target_id: &str,
        client_id: &str,
    ) -> Result<i64> {
        let mut conn = self
            .redis_pool
            .get()
            .await
            .context("failed to acquire redis connection for total call lookup")?;

        let total_key = format!("pulse-gate:stats:target:{}:clients", target_id);
        let value: i64 = redis::cmd("HGET")
            .arg(&[&total_key, client_id])
            .query_async(&mut conn)
            .await
            .unwrap_or(0);

        Ok(value)
    }

    pub async fn get_current_minute_calls_for_client(
        &self,
        target_id: &str,
        client_id: &str,
    ) -> Result<i64> {
        let mut conn = self
            .redis_pool
            .get()
            .await
            .context("failed to acquire redis connection for minute bucket lookup")?;

        let bucket = Utc::now().format("%Y%m%d%H%M").to_string();
        let rate_key = format!(
            "pulse-gate:rate:target:{}:client:{}:minute:{}",
            target_id, client_id, bucket
        );

        let value: i64 = redis::cmd("GET")
            .arg(&rate_key)
            .query_async(&mut conn)
            .await
            .unwrap_or(0);

        Ok(value)
    }

    pub async fn get_usage_snapshot_for_client(
        &self,
        target_id: &str,
        client_id: &str,
    ) -> Result<TargetClientUsageSnapshot> {
        let total_calls = self
            .get_total_calls_for_client(target_id, client_id)
            .await?;
        let calls_in_current_minute = self
            .get_current_minute_calls_for_client(target_id, client_id)
            .await?;
        let current_minute_bucket = Utc::now().format("%Y%m%d%H%M").to_string();

        Ok(TargetClientUsageSnapshot {
            target_id: target_id.to_string(),
            client_id: client_id.to_string(),
            total_calls,
            calls_in_current_minute,
            current_minute_bucket,
        })
    }

    pub async fn clear_all(&self) -> Result<()> {
        let mut conn = self
            .redis_pool
            .get()
            .await
            .context("failed to acquire redis connection for call stats reset")?;

        let _: () = redis::cmd("FLUSHDB")
            .query_async(&mut conn)
            .await
            .context("failed to flush Redis database for call stats reset")?;

        debug!("Cleared all pulse-gate Redis call stats and rate-limit keys");
        Ok(())
    }
}
