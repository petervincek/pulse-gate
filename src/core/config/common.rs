use std::{env, fs, path::PathBuf};

use crate::core::config::keycloak::{
    KEYCLOAK_EXPECTED_AUDIENCE, KEYCLOAK_JWKS_REFRESH_LOOP_INTERVAL_SEC, KEYCLOAK_REALM_URL,
    KeycloakConfig,
};
use crate::core::config::postgres::{
    POSTGRES_ACQUIRE_TIMEOUT_MS, POSTGRES_CONNECT_TIMEOUT_MS, POSTGRES_IDLE_TIMEOUT_SECS,
    POSTGRES_MAX_CONNECTIONS, POSTGRES_MAX_LIFETIME_SECS, POSTGRES_MIN_CONNECTIONS,
    POSTGRES_SSL_MODE, POSTGRES_STATEMENT_TIMEOUT_MS, POSTGRES_URL, POSTGRES_VALIDATE_ON_STARTUP,
    PostgresConfig,
};
use crate::core::config::redis::{
    REDIS_POOL_MAX_SIZE, REDIS_POOL_RECYCLE_SECONDS, REDIS_POOL_TIMEOUT_MS,
    REDIS_POOL_WAIT_TIMEOUT_MS, REDIS_URL, REDIS_VALIDATE_ON_STARTUP, RedisConfig,
};
use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// `AppConfig` contains the whole app configuration
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub logging_config: LoggingConfig,
    #[serde(default)]
    pub redis_config: RedisConfig,
    #[serde(default)]
    pub postgres_config: PostgresConfig,
    #[serde(default)]
    pub keycloak_config: KeycloakConfig,
}

impl AppConfig {
    pub fn merge_with_env(self) -> Self {
        self.merge_with_redis_env()
            .merge_with_postgres_env()
            .merge_with_keycloak_env()
    }

    fn merge_with_redis_env(mut self) -> Self {
        if let Ok(url) = env::var(REDIS_URL) {
            self.redis_config.url = url;
        }

        self.redis_config.pool_max_size = env::var(REDIS_POOL_MAX_SIZE)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.redis_config.pool_max_size);

        self.redis_config.pool_timeout_ms = env::var(REDIS_POOL_TIMEOUT_MS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.redis_config.pool_timeout_ms);

        self.redis_config.pool_wait_timeout_ms = env::var(REDIS_POOL_WAIT_TIMEOUT_MS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.redis_config.pool_wait_timeout_ms);

        self.redis_config.pool_recycle_seconds = env::var(REDIS_POOL_RECYCLE_SECONDS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.redis_config.pool_recycle_seconds);

        self.redis_config.validate_on_startup = env::var(REDIS_VALIDATE_ON_STARTUP)
            .ok()
            .and_then(|value| {
                if value == "1" || value.eq_ignore_ascii_case("true") {
                    Some(true)
                } else if value == "0" || value.eq_ignore_ascii_case("false") {
                    Some(false)
                } else {
                    None
                }
            })
            .unwrap_or(self.redis_config.validate_on_startup);

        self
    }

    fn merge_with_postgres_env(mut self) -> Self {
        if let Ok(url) = env::var(POSTGRES_URL) {
            self.postgres_config.url = url;
        }

        // pool sizing
        self.postgres_config.max_connections = env::var(POSTGRES_MAX_CONNECTIONS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.postgres_config.max_connections);
        self.postgres_config.min_connections = env::var(POSTGRES_MIN_CONNECTIONS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.postgres_config.min_connections);

        // timeouts
        self.postgres_config.connect_timeout_ms = env::var(POSTGRES_CONNECT_TIMEOUT_MS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.postgres_config.connect_timeout_ms);
        self.postgres_config.acquire_timeout_ms = env::var(POSTGRES_ACQUIRE_TIMEOUT_MS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.postgres_config.acquire_timeout_ms);
        self.postgres_config.idle_timeout_secs = env::var(POSTGRES_IDLE_TIMEOUT_SECS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.postgres_config.idle_timeout_secs);
        self.postgres_config.max_lifetime_secs = env::var(POSTGRES_MAX_LIFETIME_SECS)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.postgres_config.max_lifetime_secs);

        // startup validation
        self.postgres_config.validate_on_startup = env::var(POSTGRES_VALIDATE_ON_STARTUP)
            .ok()
            .and_then(|value| {
                if value == "1" || value.eq_ignore_ascii_case("true") {
                    Some(true)
                } else if value == "0" || value.eq_ignore_ascii_case("false") {
                    Some(false)
                } else {
                    None
                }
            })
            .unwrap_or(self.postgres_config.validate_on_startup);

        // ssl/tls
        self.postgres_config.ssl_mode = env::var(POSTGRES_SSL_MODE)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.postgres_config.ssl_mode);

        // statement timeouts
        self.postgres_config.statement_timeout_ms = env::var(POSTGRES_STATEMENT_TIMEOUT_MS)
            .ok()
            .and_then(|value| match value.parse::<u64>() {
                Ok(timeout) => Some(Some(timeout)),
                Err(_) => None,
            })
            .unwrap_or(self.postgres_config.statement_timeout_ms);

        self
    }

    fn merge_with_keycloak_env(mut self) -> Self {
        if let Ok(url) = env::var(KEYCLOAK_REALM_URL) {
            self.keycloak_config.url = url;
        }

        if let Ok(expected_audience) = env::var(KEYCLOAK_EXPECTED_AUDIENCE)
            && expected_audience.len() > 0
        {
            self.keycloak_config.expected_audience = Some(expected_audience);
        }

        self.keycloak_config.jwks_refresh_loop_interval_sec =
            env::var(KEYCLOAK_JWKS_REFRESH_LOOP_INTERVAL_SEC)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(self.keycloak_config.jwks_refresh_loop_interval_sec);

        self
    }
}

/// `LoggingConfig` contains logging specific configuration
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct LoggingConfig {
    pub log_file_name: String,
    pub log_level: String,
    pub max_log_files: usize,
    pub max_log_file_size_mb: usize,
}

/// Provides default starting values for logging configuration
impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_file_name: String::from("pulse-gate.log"),
            log_level: String::from("info"),
            max_log_files: 14,
            max_log_file_size_mb: 10,
        }
    }
}

#[derive(Debug, Default)]
pub struct ConfigManager {
    config_dir: Option<PathBuf>,
}

/// `ConfigManager` is responsible for getting and loading the configuration
impl ConfigManager {
    // creates new instance of config manager
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        Self { config_dir }
    }

    // get the native OS path of the configuration directory for this tool
    pub fn get_config_dir(&self) -> PathBuf {
        match &self.config_dir {
            None => ProjectDirs::from("org", "vincek", "pulse-gate")
                .map(|proj| proj.config_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from("./config")),
            Some(provided_config_dir) => provided_config_dir.clone(),
        }
    }

    /// `get_config_file_path` return the path to the main configuration file for this application
    pub fn get_config_file_path(&self) -> PathBuf {
        self.get_config_dir().join("config.toml")
    }

    /// Loads configuration from disk or generates a fallback template if empty
    /// initially no support to override this configuration with values
    /// from environment variables or command line flags/arguments
    /// just raw parsing of the file without any business related logic like validation
    pub fn load_or_create(&self) -> anyhow::Result<AppConfig> {
        let config_dir = self.get_config_dir();
        fs::create_dir_all(&config_dir)?;

        let config_file = self.get_config_file_path();

        if !config_file.exists() {
            // Seed an empty configuration template file with default values
            let default_config = AppConfig::default();

            let toml_string = toml::to_string_pretty(&default_config)?;
            fs::write(&config_file, toml_string)?;
            return Ok(default_config);
        }

        let content = fs::read_to_string(config_file)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// `save_config` saves/persists the provided application config to the config file
    pub fn save_config(&self, app_config: &AppConfig) -> Result<()> {
        let toml_string = toml::to_string_pretty(app_config)?;
        let config_file = self.get_config_file_path();
        fs::write(&config_file, toml_string)?;
        Ok(())
    }
}
