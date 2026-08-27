use std::{fs, path::PathBuf};

use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// `AppConfig` contains the whole app configuration
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub logging_config: LoggingConfig,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn temp_config_manager() -> (tempfile::TempDir, ConfigManager) {
        let dir = tempdir().expect("failed to create temp dir");
        let manager = ConfigManager::new(Some(dir.path().to_path_buf()));
        (dir, manager)
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
}
