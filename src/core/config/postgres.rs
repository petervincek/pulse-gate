use serde::{Deserialize, Serialize};

// POSTGRESQL env variables
pub const POSTGRES_URL: &str = "POSTGRES_URL";
pub const POSTGRES_MAX_CONNECTIONS: &str = "POSTGRES_MAX_CONNECTIONS";
pub const POSTGRES_MIN_CONNECTIONS: &str = "POSTGRES_MIN_CONNECTIONS";
pub const POSTGRES_CONNECT_TIMEOUT_MS: &str = "POSTGRES_CONNECT_TIMEOUT_MS";
pub const POSTGRES_ACQUIRE_TIMEOUT_MS: &str = "POSTGRES_ACQUIRE_TIMEOUT_MS";
pub const POSTGRES_IDLE_TIMEOUT_SECS: &str = "POSTGRES_IDLE_TIMEOUT_SECS";
pub const POSTGRES_MAX_LIFETIME_SECS: &str = "POSTGRES_MAX_LIFETIME_SECS";
pub const POSTGRES_VALIDATE_ON_STARTUP: &str = "POSTGRES_VALIDATE_ON_STARTUP";
pub const POSTGRES_SSL_MODE: &str = "POSTGRES_SSL_MODE";
pub const POSTGRES_STATEMENT_TIMEOUT_MS: &str = "POSTGRES_STATEMENT_TIMEOUT_MS";

/// `PostgresConfig` contains Postgres database specific configuration
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct PostgresConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_ms: u64,
    pub acquire_timeout_ms: u64,
    pub idle_timeout_secs: u64,
    pub max_lifetime_secs: u64,
    pub validate_on_startup: bool,
    pub ssl_mode: String,
    pub statement_timeout_ms: Option<u64>,
}

/// Provides default starting values for postgres configuration
impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            // connection url/string
            url: String::from(
                "postgres://pulse_gate:pulse_gate_password@127.0.0.1:5432/pulse_gate_dev",
            ),
            // pool sizing
            max_connections: 16, // starting point
            min_connections: 4,  // small warm pool - to lower the latency
            // timeout config
            connect_timeout_ms: 5000,
            acquire_timeout_ms: 3000,
            idle_timeout_secs: 600,
            max_lifetime_secs: 1800,
            // startup validation
            validate_on_startup: true,
            // SSL/TLS (later we can support 'sslrootcert' and client certs)
            ssl_mode: String::from("prefer"),
            // statement timeouts - to avoid runaway queries
            statement_timeout_ms: Some(30000),
        }
    }
}
