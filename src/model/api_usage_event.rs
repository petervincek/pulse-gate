use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, prelude::FromRow};
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiUsageEvent {
    pub client_id: String,
    pub target_id: String,
    pub route_prefix: String,
    pub method: String,
    pub status: String, // accepted, success, rejected, rate_limited, upstream_error
    pub response_code: Option<i32>,
    pub upstream_host: Option<String>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

pub struct UsageEventRepo {
    pg_pool: PgPool,
}

impl UsageEventRepo {
    pub fn new(pg_pool: PgPool) -> Self {
        Self { pg_pool }
    }

    pub async fn insert(&self, event: &ApiUsageEvent) -> Result<ApiUsageEvent> {
        let created_event = sqlx::query_as::<_, ApiUsageEvent>(
            r#"
            INSERT INTO api_usage_event (
                client_id,
                target_id,
                route_prefix,
                method,
                status,
                response_code,
                upstream_host,
                occurred_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING
                client_id,
                target_id,
                route_prefix,
                method,
                status,
                response_code,
                upstream_host,
                occurred_at,
                created_at
            "#,
        )
        .bind(&event.client_id)
        .bind(&event.target_id)
        .bind(&event.route_prefix)
        .bind(&event.method)
        .bind(&event.status)
        .bind(event.response_code)
        .bind(&event.upstream_host)
        .bind(&event.occurred_at)
        .fetch_one(&self.pg_pool)
        .await?;

        Ok(created_event)
    }

    pub async fn clear_all(&self) -> Result<()> {
        sqlx::query("TRUNCATE TABLE api_usage_event")
            .execute(&self.pg_pool)
            .await?;
        Ok(())
    }
}

pub struct UsageEventBus {
    tx: Sender<ApiUsageEvent>,
}

impl UsageEventBus {
    pub fn new(tx: Sender<ApiUsageEvent>) -> Self {
        Self { tx }
    }

    pub async fn emit(&self, event: ApiUsageEvent) {
        let _ = self.tx.send(event).await;
    }
}

pub async fn run_usage_event_worker(mut rx: Receiver<ApiUsageEvent>, repo: UsageEventRepo) {
    while let Some(event) = rx.recv().await {
        if let Err(err) = repo.insert(&event).await {
            tracing::warn!(
                error = %err,
                client_id = %event.client_id,
                target_id = %event.target_id,
                "failed to persist usage event"
            );
        }
    }
}
