use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, prelude::FromRow};

pub type PathPrefix = String;

/// `RouteTarget` represents the target definition for a dynamic service route
#[derive(Debug, Serialize, Deserialize, FromRow, PartialEq, Eq, Clone)]
pub struct RouteTarget {
    pub path_prefix: String,
    pub upstream_base_url: String,
    pub rate_limit_per_min: i32,
    pub required_role: Option<String>,
}

impl RouteTarget {
    pub fn new(
        path_prefix: String,
        upstream_base_url: String,
        rate_limit_per_min: i32,
        required_role: Option<String>,
    ) -> Self {
        Self {
            path_prefix,
            upstream_base_url,
            rate_limit_per_min,
            required_role,
        }
    }
}

/// `RouteTargetRepo` represents the repository service layer responsible for managing the `RouteTarget` entities in the db
#[derive(Debug)]
pub struct RouteTargetRepo {
    pg_pool: PgPool, // reference to the DB connection pool
}

impl RouteTargetRepo {
    pub fn new(pg_pool: PgPool) -> Self {
        Self { pg_pool }
    }

    pub async fn create_route_target(&self, route_target: RouteTarget) -> Result<RouteTarget> {
        let created_route_target = sqlx::query_as::<_, RouteTarget>(
            r#"
            INSERT INTO route_target (path_prefix, upstream_base_url, rate_limit_per_min, required_role)
            VALUES ($1, $2, $3, $4)
            RETURNING path_prefix, upstream_base_url, rate_limit_per_min, required_role
            "#,
        )
        .bind(&route_target.path_prefix)
        .bind(&route_target.upstream_base_url)
        .bind(route_target.rate_limit_per_min)
        .bind(&route_target.required_role)
        .fetch_one(&self.pg_pool)
        .await?;
        Ok(created_route_target)
    }

    pub async fn get_route_target_by_path_prefix(&self, path_prefix: &str) -> Result<RouteTarget> {
        sqlx::query_as::<_, RouteTarget>(
            r#"
            SELECT path_prefix, upstream_base_url, rate_limit_per_min, required_role
            FROM route_target
            WHERE path_prefix = $1
            "#,
        )
        .bind(path_prefix)
        .fetch_optional(&self.pg_pool)
        .await?
        .ok_or(anyhow::anyhow!(
            "Route Target with path_prefix: {path_prefix} not found"
        ))
    }

    pub async fn list_route_targets(&self) -> Result<Vec<RouteTarget>> {
        let route_targets = sqlx::query_as::<_, RouteTarget>(
            r#"
            SELECT path_prefix, upstream_base_url, rate_limit_per_min, required_role
            FROM route_target
            ORDER BY path_prefix ASC
            "#,
        )
        .fetch_all(&self.pg_pool)
        .await?;
        Ok(route_targets)
    }

    pub async fn update_route_target(&self, route_target: RouteTarget) -> Result<RouteTarget> {
        sqlx::query_as::<_, RouteTarget>(
            r#"
            UPDATE route_target
            SET upstream_base_url = $1, rate_limit_per_min = $2, required_role = $3, updated_at = now()
            WHERE path_prefix = $4
            RETURNING path_prefix, upstream_base_url, rate_limit_per_min, required_role
            "#,
        )
        .bind(&route_target.upstream_base_url)
        .bind(route_target.rate_limit_per_min)
        .bind(&route_target.required_role)
        .bind(&route_target.path_prefix)
        .fetch_optional(&self.pg_pool)
        .await?
        .ok_or(anyhow::anyhow!(
            "Route Target with path_prefix: {} not found",
            route_target.path_prefix
        ))
    }

    pub async fn delete_route_target_by_path_prefix(&self, path_prefix: &str) -> Result<()> {
        let result = sqlx::query(
            r#"
            DELETE FROM route_target
            WHERE path_prefix = $1
            "#,
        )
        .bind(path_prefix)
        .execute(&self.pg_pool)
        .await?;

        if result.rows_affected() == 0 {
            Err(anyhow::anyhow!(
                "Route Target with path_prefix: {path_prefix} not found"
            ))
        } else {
            Ok(())
        }
    }
}
