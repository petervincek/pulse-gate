use std::sync::Arc;

use anyhow::{Context, Result};
use dashmap::DashMap;
use deadpool_redis::Pool as RedisPool;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::model::route_target::{PathPrefix, RouteTarget};

pub const ROUTE_TARGET_EVENTS_CHANNEL: &str = "pulse-gate:route-events";
pub const ROUTE_TARGET_VERSIONS_HASH: &str = "pulse-gate:route-versions";

/// `RouteTargetEventRepo` represents the repository for handling the events around route targets
#[derive(Debug, Clone)]
pub struct RouteTargetEventRepo {
    redis_pool: RedisPool,
}

/// `RouteTargetOperation` represents one of several action that can happen while managing the route targets
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
pub enum RouteTargetOperation {
    Created,
    Updated,
    Deleted,
}

/// `RouteTargetEvent` represents the data structure that holds the actual event data
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct RouteTargetEvent {
    pub version: u64,
    pub operation: RouteTargetOperation,
    pub path_prefix: Option<String>,
    pub route_target: Option<RouteTarget>,
}

impl RouteTargetEvent {
    pub fn created(route_target: RouteTarget, version: u64) -> Self {
        Self {
            version,
            operation: RouteTargetOperation::Created,
            path_prefix: Some(route_target.path_prefix.clone()),
            route_target: Some(route_target),
        }
    }

    pub fn updated(route_target: RouteTarget, version: u64) -> Self {
        Self {
            version,
            operation: RouteTargetOperation::Updated,
            path_prefix: Some(route_target.path_prefix.clone()),
            route_target: Some(route_target),
        }
    }

    pub fn deleted(path_prefix: String, version: u64) -> Self {
        Self {
            version,
            operation: RouteTargetOperation::Deleted,
            path_prefix: Some(path_prefix),
            route_target: None,
        }
    }
}

impl RouteTargetEventRepo {
    pub fn new(redis_pool: RedisPool) -> Self {
        Self { redis_pool }
    }

    pub async fn current_route_version(&self, path_prefix: &str) -> Result<u64> {
        // get the connection to Redis
        let mut conn = self
            .redis_pool
            .get()
            .await
            .context("failed to acquire redis connection for route version lookup")?;

        // try to get the actual version for the route target's prefix (sort of id for this entity)
        let version: Option<u64> = redis::cmd("HGET")
            .arg(&[ROUTE_TARGET_VERSIONS_HASH, path_prefix])
            .query_async(&mut conn)
            .await
            .context("failed to read route version from Redis")?;

        Ok(version.unwrap_or_default())
    }

    pub async fn set_route_version(&self, path_prefix: &str, version: u64) -> Result<()> {
        // get the connection to Redis
        let mut conn = self
            .redis_pool
            .get()
            .await
            .context("failed to acquire redis connection for route version seeding")?;

        // set the version
        let _: u64 = redis::cmd("HSET")
            .arg(&[
                ROUTE_TARGET_VERSIONS_HASH,
                path_prefix,
                &version.to_string(),
            ])
            .query_async(&mut conn)
            .await
            .context("failed to seed route version in Redis")?;

        Ok(())
    }

    pub async fn hydrate_route_versions(
        &self,
        service_routes: &DashMap<PathPrefix, RouteTarget>,
        route_versions: &DashMap<PathPrefix, u64>,
    ) -> Result<()> {
        for route in service_routes.iter() {
            let version = self.current_route_version(&route.path_prefix).await?;
            route_versions.insert(route.path_prefix.clone(), version);
            self.set_route_version(&route.path_prefix, version).await?;
        }

        Ok(())
    }

    pub async fn next_route_version(&self, path_prefix: &str) -> Result<u64> {
        // get the connection to Redis
        let mut conn = self
            .redis_pool
            .get()
            .await
            .context("failed to acquire redis connection for route version generation")?;

        let version: u64 = redis::cmd("HINCRBY")
            .arg(&[ROUTE_TARGET_VERSIONS_HASH, path_prefix, "1"])
            .query_async(&mut conn)
            .await
            .context("failed to increment route version in Redis")?;

        Ok(version)
    }

    pub async fn publish_route_event(&self, event: &RouteTargetEvent) -> Result<()> {
        // get the connection to Redis
        let mut conn = self
            .redis_pool
            .get()
            .await
            .context("failed to acquire redis connection for route event publication")?;

        let payload = serde_json::to_string(event)
            .context("failed to serialize route target event for Redis publication")?;

        // publish the event to Redis
        let _: i64 = redis::cmd("PUBLISH")
            .arg(&[ROUTE_TARGET_EVENTS_CHANNEL, &payload])
            .query_async(&mut conn)
            .await
            .context("failed to publish route target event to Redis")?;

        Ok(())
    }

    pub async fn start_route_change_listener(
        &self,
        service_routes: Arc<DashMap<PathPrefix, RouteTarget>>,
        route_versions: Arc<DashMap<PathPrefix, u64>>,
        redis_url: &str,
    ) -> Result<()> {
        let client = redis::Client::open(redis_url)?;
        let mut pubsub = client.get_async_pubsub().await?;
        pubsub.subscribe(ROUTE_TARGET_EVENTS_CHANNEL).await?;

        // start the listener in separate thread
        tokio::spawn(async move {
            let mut messages = pubsub.on_message();

            while let Some(message) = messages.next().await {
                let payload: String = match message.get_payload() {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::error!(error = ?err, "failed to read route target event payload");
                        continue;
                    }
                };

                let event: RouteTargetEvent = match serde_json::from_str(&payload) {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::error!(error = ?err, payload, "failed to deserialize route target event");
                        continue;
                    }
                };

                match event.operation {
                    RouteTargetOperation::Created | RouteTargetOperation::Updated => {
                        if let Some(route_target) = &event.route_target {
                            let path_prefix = route_target.path_prefix.clone();
                            let current_version = route_versions
                                .get(&path_prefix)
                                .map(|version| *version)
                                .unwrap_or_default();

                            if event.version <= current_version {
                                tracing::debug!(
                                    path_prefix,
                                    event_version = event.version,
                                    current_version,
                                    "ignoring stale route event"
                                );
                                continue;
                            }

                            route_versions.insert(path_prefix.clone(), event.version);
                            service_routes.insert(path_prefix, route_target.clone());
                        }
                    }
                    RouteTargetOperation::Deleted => {
                        if let Some(path_prefix) = &event.path_prefix {
                            let current_version = route_versions
                                .get(path_prefix)
                                .map(|version| *version)
                                .unwrap_or_default();

                            if event.version <= current_version {
                                tracing::debug!(
                                    path_prefix,
                                    event_version = event.version,
                                    current_version,
                                    "ignoring stale route delete event"
                                );
                                continue;
                            }

                            route_versions.insert(path_prefix.clone(), event.version);
                            service_routes.remove(path_prefix);
                        }
                    }
                }
            }
        });

        Ok(())
    }
}
