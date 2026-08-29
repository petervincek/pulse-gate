use serde::{Deserialize, Serialize};

// REDIS env variables
pub const REDIS_URL: &str = "REDIS_URL";
pub const REDIS_POOL_MAX_SIZE: &str = "REDIS_POOL_MAX_SIZE";
pub const REDIS_POOL_TIMEOUT_MS: &str = "REDIS_POOL_TIMEOUT_MS";
pub const REDIS_POOL_WAIT_TIMEOUT_MS: &str = "REDIS_POOL_WAIT_TIMEOUT_MS";
pub const REDIS_POOL_RECYCLE_SECONDS: &str = "REDIS_POOL_RECYCLE_SECONDS";
pub const REDIS_VALIDATE_ON_STARTUP: &str = "REDIS_VALIDATE_ON_STARTUP";

/// `RedisConfig` contains redis database specific configuration
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct RedisConfig {
    pub url: String,
    pub pool_max_size: usize,
    pub pool_timeout_ms: u64,
    pub pool_wait_timeout_ms: u64,
    pub pool_recycle_seconds: u64,
    pub validate_on_startup: bool,
}

/// Provides default starting values for logging configuration
impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: String::from("redis://127.0.0.1:6379/0"),
            pool_max_size: 16,
            pool_timeout_ms: 3000,
            pool_wait_timeout_ms: 500,
            pool_recycle_seconds: 3600,
            validate_on_startup: true,
        }
    }
}
