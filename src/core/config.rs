use std::{env, fs, path::PathBuf};

use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const REDIS_URL: &str = "REDIS_URL";
const REDIS_POOL_MAX_SIZE: &str = "REDIS_POOL_MAX_SIZE";
const REDIS_POOL_TIMEOUT_MS: &str = "REDIS_POOL_TIMEOUT_MS";
const REDIS_POOL_WAIT_TIMEOUT_MS: &str = "REDIS_POOL_WAIT_TIMEOUT_MS";
const REDIS_POOL_RECYCLE_SECONDS: &str = "REDIS_POOL_RECYCLE_SECONDS";
const REDIS_VALIDATE_ON_STARTUP: &str = "REDIS_VALIDATE_ON_STARTUP";

/// `AppConfig` contains the whole app configuration
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub logging_config: LoggingConfig,
    #[serde(default)]
    pub redis_config: RedisConfig,
}

impl AppConfig {
    pub fn merge_with_env(mut self) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, sync::Mutex};
    use tempfile::tempdir;

    // in case of paralel test runs we need to make sure that one test is not negatively affecting another
    // so sync with potential tests that may be affected with this lock
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env_lock<T>(test_to_run: impl FnOnce() -> T) -> T {
        // keep the guard through the run of this function and run the test, the guard will be dropped at the end
        let _guard = ENV_LOCK.lock().expect("failed to lock env mutex");
        test_to_run()
    }

    fn temp_config_manager() -> (tempfile::TempDir, ConfigManager) {
        let dir = tempdir().expect("failed to create temp dir");
        let manager = ConfigManager::new(Some(dir.path().to_path_buf()));
        (dir, manager)
    }

    fn clear_redis_env_vars() {
        unsafe {
            env::remove_var(REDIS_URL);
            env::remove_var(REDIS_POOL_MAX_SIZE);
            env::remove_var(REDIS_POOL_TIMEOUT_MS);
            env::remove_var(REDIS_POOL_WAIT_TIMEOUT_MS);
            env::remove_var(REDIS_POOL_RECYCLE_SECONDS);
            env::remove_var(REDIS_VALIDATE_ON_STARTUP);
        }
    }

    #[test]
    fn load_or_create_creates_default_configuration_file() {
        let (_dir, manager) = temp_config_manager();

        let config_file = manager.get_config_file_path();
        assert!(!config_file.exists());

        let _config = manager.load_or_create().expect("load_or_create failed");

        assert!(config_file.exists());
    }

    #[test]
    fn load_or_create_reads_existing_configuration_file() {
        let (_dir, manager) = temp_config_manager();
        let config_file = manager.get_config_file_path();
        fs::create_dir_all(config_file.parent().expect("missing parent dir")).unwrap();

        let expected = AppConfig::default();

        let toml_string = toml::to_string_pretty(&expected).expect("serialize failed");
        fs::write(&config_file, toml_string).expect("write config file failed");

        let actual = manager.load_or_create().expect("load_or_create failed");

        assert_eq!(actual, expected);
    }

    #[test]
    fn save_config_persists_configuration_to_disk() {
        let (_dir, manager) = temp_config_manager();
        let config = AppConfig::default();

        manager.save_config(&config).expect("save_config failed");

        let actual_contents =
            fs::read_to_string(manager.get_config_file_path()).expect("read config failed");
        let actual_config: AppConfig =
            toml::from_str(&actual_contents).expect("parse config failed");

        assert_eq!(actual_config, config);
    }

    #[test]
    fn save_config_writes_modified_values_to_file() {
        let dir = tempdir().expect("failed to create temp dir");
        let manager = ConfigManager::new(Some(dir.path().to_path_buf()));

        let mut config = AppConfig::default();
        config.redis_config.url = "redis://example.test:6379/2".to_string();
        config.redis_config.pool_max_size = 42;
        config.logging_config.log_level = "debug".to_string();

        manager.save_config(&config).expect("save_config failed");

        let actual_contents =
            fs::read_to_string(manager.get_config_file_path()).expect("read config failed");
        let actual_config: AppConfig =
            toml::from_str(&actual_contents).expect("parse config failed");

        assert_eq!(
            actual_config.redis_config.url,
            "redis://example.test:6379/2"
        );
        assert_eq!(actual_config.redis_config.pool_max_size, 42);
        assert_eq!(actual_config.logging_config.log_level, "debug");
    }

    #[test]
    fn save_config_roundtrip_preserves_values() {
        let dir = tempdir().expect("failed to create temp dir");
        let manager = ConfigManager::new(Some(dir.path().to_path_buf()));

        let mut expected = AppConfig::default();
        expected.redis_config.url = "redis://example.test:6379/3".to_string();
        expected.redis_config.pool_recycle_seconds = 12345;
        expected.logging_config.log_file_name = "roundtrip.log".to_string();

        manager.save_config(&expected).expect("save_config failed");
        let actual = manager.load_or_create().expect("load_or_create failed");

        assert_eq!(actual, expected);
    }

    #[test]
    fn merge_with_env_accepts_true_for_validate_on_startup() {
        with_env_lock(|| {
            clear_redis_env_vars();
            unsafe {
                env::set_var(REDIS_VALIDATE_ON_STARTUP, "TRUE");
            }

            let original = AppConfig::default();
            let merged = original.merge_with_env();

            assert!(merged.redis_config.validate_on_startup);

            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_accepts_one_for_validate_on_startup() {
        with_env_lock(|| {
            clear_redis_env_vars();
            unsafe {
                env::set_var(REDIS_VALIDATE_ON_STARTUP, "1");
            }

            let original = AppConfig::default();
            let merged = original.merge_with_env();

            assert!(merged.redis_config.validate_on_startup);

            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_rejects_invalid_boolean_values_for_validate_on_startup() {
        with_env_lock(|| {
            clear_redis_env_vars();
            unsafe {
                env::set_var(REDIS_VALIDATE_ON_STARTUP, "maybe");
            }

            let original = AppConfig::default();
            let merged = original.clone().merge_with_env();

            assert_eq!(
                merged.redis_config.validate_on_startup,
                original.redis_config.validate_on_startup,
            );

            clear_redis_env_vars();
        });
    }

    #[test]
    fn save_config_fails_if_config_file_is_missing() {
        let dir = tempdir().expect("failed to create temp dir");
        let nested_dir = dir.path().join("nested").join("config-dir");
        let manager = ConfigManager::new(Some(nested_dir.clone()));

        assert!(!nested_dir.exists());

        let result = manager.save_config(&AppConfig::default());

        assert!(result.is_err());
    }

    #[test]
    fn get_config_file_path_uses_override_directory() {
        let (_dir, manager) = temp_config_manager();
        let config_file = manager.get_config_file_path();

        assert_eq!(config_file.parent().unwrap(), manager.get_config_dir());
        assert_eq!(config_file.file_name().unwrap(), "config.toml");
    }

    #[test]
    fn get_config_dir_uses_the_explicit_override() {
        let dir = tempdir().expect("failed to create temp dir");
        let override_dir = dir.path().join("explicit-config");
        let manager = ConfigManager::new(Some(override_dir.clone()));

        assert_eq!(manager.get_config_dir(), override_dir);
    }

    #[test]
    fn merge_with_env_overrides_redis_values() {
        with_env_lock(|| {
            clear_redis_env_vars();
            unsafe {
                env::set_var(REDIS_URL, "redis://example.com:6379/1");
                env::set_var(REDIS_POOL_MAX_SIZE, "32");
                env::set_var(REDIS_POOL_TIMEOUT_MS, "4500");
                env::set_var(REDIS_POOL_WAIT_TIMEOUT_MS, "800");
                env::set_var(REDIS_POOL_RECYCLE_SECONDS, "7200");
                env::set_var(REDIS_VALIDATE_ON_STARTUP, "false");
            }

            let original = AppConfig::default();
            let merged = original.merge_with_env();

            assert_eq!(merged.redis_config.url, "redis://example.com:6379/1");
            assert_eq!(merged.redis_config.pool_max_size, 32);
            assert_eq!(merged.redis_config.pool_timeout_ms, 4500);
            assert_eq!(merged.redis_config.pool_wait_timeout_ms, 800);
            assert_eq!(merged.redis_config.pool_recycle_seconds, 7200);
            assert_eq!(merged.redis_config.validate_on_startup, false);

            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_ignores_invalid_numeric_values() {
        with_env_lock(|| {
            clear_redis_env_vars();
            unsafe {
                env::set_var(REDIS_POOL_MAX_SIZE, "not-a-number");
                env::set_var(REDIS_POOL_TIMEOUT_MS, "n/a");
                env::set_var(REDIS_POOL_WAIT_TIMEOUT_MS, "bad");
                env::set_var(REDIS_POOL_RECYCLE_SECONDS, "invalid");
                env::set_var(REDIS_VALIDATE_ON_STARTUP, "0");
            }

            let original = AppConfig::default();
            let merged = original.clone().merge_with_env();

            assert_eq!(
                merged.redis_config.pool_max_size,
                original.redis_config.pool_max_size
            );
            assert_eq!(
                merged.redis_config.pool_timeout_ms,
                original.redis_config.pool_timeout_ms
            );
            assert_eq!(
                merged.redis_config.pool_wait_timeout_ms,
                original.redis_config.pool_wait_timeout_ms
            );
            assert_eq!(
                merged.redis_config.pool_recycle_seconds,
                original.redis_config.pool_recycle_seconds
            );
            assert_eq!(merged.redis_config.validate_on_startup, false);

            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_preserves_logging_config() {
        with_env_lock(|| {
            clear_redis_env_vars();
            unsafe {
                env::set_var(REDIS_URL, "redis://example.com:6379/1");
                env::set_var(REDIS_POOL_MAX_SIZE, "32");
            }

            let mut original = AppConfig::default();
            original.logging_config.log_level = "debug".to_string();
            original.logging_config.log_file_name = "custom.log".to_string();

            let merged = original.clone().merge_with_env();

            assert_eq!(merged.logging_config, original.logging_config);
            assert_eq!(merged.redis_config.url, "redis://example.com:6379/1");
            assert_eq!(merged.redis_config.pool_max_size, 32);

            clear_redis_env_vars();
        });
    }
}
