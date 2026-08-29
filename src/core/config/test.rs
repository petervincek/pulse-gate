#[cfg(test)]
mod tests {

    use crate::core::config::{
        common::{AppConfig, ConfigManager},
        keycloak::KEYCLOAK_REALM_URL,
        postgres::{
            POSTGRES_ACQUIRE_TIMEOUT_MS, POSTGRES_CONNECT_TIMEOUT_MS, POSTGRES_IDLE_TIMEOUT_SECS,
            POSTGRES_MAX_CONNECTIONS, POSTGRES_MAX_LIFETIME_SECS, POSTGRES_MIN_CONNECTIONS,
            POSTGRES_SSL_MODE, POSTGRES_STATEMENT_TIMEOUT_MS, POSTGRES_URL,
            POSTGRES_VALIDATE_ON_STARTUP,
        },
        redis::{
            REDIS_POOL_MAX_SIZE, REDIS_POOL_RECYCLE_SECONDS, REDIS_POOL_TIMEOUT_MS,
            REDIS_POOL_WAIT_TIMEOUT_MS, REDIS_URL, REDIS_VALIDATE_ON_STARTUP,
        },
    };
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

    fn clear_postgres_env_vars() {
        unsafe {
            env::remove_var(POSTGRES_URL);
            env::remove_var(POSTGRES_MAX_CONNECTIONS);
            env::remove_var(POSTGRES_MIN_CONNECTIONS);
            env::remove_var(POSTGRES_CONNECT_TIMEOUT_MS);
            env::remove_var(POSTGRES_ACQUIRE_TIMEOUT_MS);
            env::remove_var(POSTGRES_IDLE_TIMEOUT_SECS);
            env::remove_var(POSTGRES_MAX_LIFETIME_SECS);
            env::remove_var(POSTGRES_VALIDATE_ON_STARTUP);
            env::remove_var(POSTGRES_SSL_MODE);
            env::remove_var(POSTGRES_STATEMENT_TIMEOUT_MS);
            env::remove_var(KEYCLOAK_REALM_URL);
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
    fn save_config_roundtrip_preserves_postgres_values() {
        let dir = tempdir().expect("failed to create temp dir");
        let manager = ConfigManager::new(Some(dir.path().to_path_buf()));

        let mut expected = AppConfig::default();
        expected.postgres_config.url = "postgres://example.test:5432/sample".to_string();
        expected.postgres_config.max_connections = 22;
        expected.postgres_config.min_connections = 5;
        expected.postgres_config.connect_timeout_ms = 9000;
        expected.postgres_config.acquire_timeout_ms = 12000;
        expected.postgres_config.idle_timeout_secs = 1500;
        expected.postgres_config.max_lifetime_secs = 7200;
        expected.postgres_config.validate_on_startup = false;
        expected.postgres_config.ssl_mode = "require".to_string();
        expected.postgres_config.statement_timeout_ms = Some(60000);

        manager.save_config(&expected).expect("save_config failed");
        let actual = manager.load_or_create().expect("load_or_create failed");

        assert_eq!(actual, expected);
    }

    #[test]
    fn merge_with_env_accepts_true_for_postgres_validate_on_startup() {
        with_env_lock(|| {
            clear_redis_env_vars();
            clear_postgres_env_vars();
            unsafe {
                env::set_var(POSTGRES_VALIDATE_ON_STARTUP, "TRUE");
            }

            let merged = AppConfig::default().merge_with_env();

            assert!(merged.postgres_config.validate_on_startup);

            clear_postgres_env_vars();
            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_rejects_invalid_boolean_values_for_postgres_validate_on_startup() {
        with_env_lock(|| {
            clear_redis_env_vars();
            clear_postgres_env_vars();
            unsafe {
                env::set_var(POSTGRES_VALIDATE_ON_STARTUP, "maybe");
            }

            let original = AppConfig::default();
            let merged = original.clone().merge_with_env();

            assert_eq!(
                merged.postgres_config.validate_on_startup,
                original.postgres_config.validate_on_startup
            );

            clear_postgres_env_vars();
            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_overrides_postgres_ssl_mode() {
        with_env_lock(|| {
            clear_redis_env_vars();
            clear_postgres_env_vars();
            unsafe {
                env::set_var(POSTGRES_SSL_MODE, "require");
            }

            let merged = AppConfig::default().merge_with_env();

            assert_eq!(merged.postgres_config.ssl_mode, "require");

            clear_postgres_env_vars();
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
            clear_postgres_env_vars();
            unsafe {
                env::set_var(REDIS_POOL_MAX_SIZE, "not-a-number");
                env::set_var(REDIS_POOL_TIMEOUT_MS, "n/a");
                env::set_var(REDIS_POOL_WAIT_TIMEOUT_MS, "bad");
                env::set_var(REDIS_POOL_RECYCLE_SECONDS, "invalid");
                env::set_var(REDIS_VALIDATE_ON_STARTUP, "0");
                env::set_var(POSTGRES_MAX_CONNECTIONS, "invalid");
                env::set_var(POSTGRES_MIN_CONNECTIONS, "bad");
                env::set_var(POSTGRES_CONNECT_TIMEOUT_MS, "xxx");
                env::set_var(POSTGRES_ACQUIRE_TIMEOUT_MS, "n/a");
                env::set_var(POSTGRES_IDLE_TIMEOUT_SECS, "oops");
                env::set_var(POSTGRES_MAX_LIFETIME_SECS, "invalid");
                env::set_var(POSTGRES_STATEMENT_TIMEOUT_MS, "not-a-number");
                env::set_var(POSTGRES_VALIDATE_ON_STARTUP, "false");
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

            assert_eq!(
                merged.postgres_config.max_connections,
                original.postgres_config.max_connections
            );
            assert_eq!(
                merged.postgres_config.min_connections,
                original.postgres_config.min_connections
            );
            assert_eq!(
                merged.postgres_config.connect_timeout_ms,
                original.postgres_config.connect_timeout_ms
            );
            assert_eq!(
                merged.postgres_config.acquire_timeout_ms,
                original.postgres_config.acquire_timeout_ms
            );
            assert_eq!(
                merged.postgres_config.idle_timeout_secs,
                original.postgres_config.idle_timeout_secs
            );
            assert_eq!(
                merged.postgres_config.max_lifetime_secs,
                original.postgres_config.max_lifetime_secs
            );
            assert_eq!(
                merged.postgres_config.statement_timeout_ms,
                original.postgres_config.statement_timeout_ms
            );
            assert_eq!(merged.postgres_config.validate_on_startup, false);

            clear_postgres_env_vars();
            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_overrides_postgres_values() {
        with_env_lock(|| {
            clear_redis_env_vars();
            clear_postgres_env_vars();
            unsafe {
                env::set_var(POSTGRES_URL, "postgres://example.com:5432/sample");
                env::set_var(POSTGRES_MAX_CONNECTIONS, "32");
                env::set_var(POSTGRES_MIN_CONNECTIONS, "6");
                env::set_var(POSTGRES_CONNECT_TIMEOUT_MS, "7000");
                env::set_var(POSTGRES_ACQUIRE_TIMEOUT_MS, "10000");
                env::set_var(POSTGRES_IDLE_TIMEOUT_SECS, "1200");
                env::set_var(POSTGRES_MAX_LIFETIME_SECS, "3600");
                env::set_var(POSTGRES_VALIDATE_ON_STARTUP, "false");
                env::set_var(POSTGRES_SSL_MODE, "require");
                env::set_var(POSTGRES_STATEMENT_TIMEOUT_MS, "45000");
            }

            let merged = AppConfig::default().merge_with_env();

            assert_eq!(
                merged.postgres_config.url,
                "postgres://example.com:5432/sample"
            );
            assert_eq!(merged.postgres_config.max_connections, 32);
            assert_eq!(merged.postgres_config.min_connections, 6);
            assert_eq!(merged.postgres_config.connect_timeout_ms, 7000);
            assert_eq!(merged.postgres_config.acquire_timeout_ms, 10000);
            assert_eq!(merged.postgres_config.idle_timeout_secs, 1200);
            assert_eq!(merged.postgres_config.max_lifetime_secs, 3600);
            assert_eq!(merged.postgres_config.validate_on_startup, false);
            assert_eq!(merged.postgres_config.ssl_mode, "require");
            assert_eq!(merged.postgres_config.statement_timeout_ms, Some(45000));

            clear_postgres_env_vars();
            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_accepts_false_for_postgres_validate_on_startup() {
        with_env_lock(|| {
            clear_redis_env_vars();
            clear_postgres_env_vars();
            unsafe {
                env::set_var(POSTGRES_VALIDATE_ON_STARTUP, "0");
            }

            let merged = AppConfig::default().merge_with_env();

            assert_eq!(merged.postgres_config.validate_on_startup, false);

            clear_postgres_env_vars();
            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_ignores_invalid_postgres_statement_timeout() {
        with_env_lock(|| {
            clear_redis_env_vars();
            clear_postgres_env_vars();
            unsafe {
                env::set_var(POSTGRES_STATEMENT_TIMEOUT_MS, "not-a-number");
            }

            let original = AppConfig::default();
            let merged = original.clone().merge_with_env();

            assert_eq!(
                merged.postgres_config.statement_timeout_ms,
                original.postgres_config.statement_timeout_ms
            );

            clear_postgres_env_vars();
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

    #[test]
    fn merge_with_env_overrides_postgres_statement_timeout() {
        with_env_lock(|| {
            clear_redis_env_vars();
            clear_postgres_env_vars();
            unsafe {
                env::set_var(POSTGRES_STATEMENT_TIMEOUT_MS, "45000");
            }

            let original = AppConfig::default();
            let merged = original.merge_with_env();

            assert_eq!(merged.postgres_config.statement_timeout_ms, Some(45000));

            clear_postgres_env_vars();
            clear_redis_env_vars();
        });
    }

    #[test]
    fn merge_with_env_overrides_keycloak_realm_url() {
        with_env_lock(|| {
            clear_redis_env_vars();
            clear_postgres_env_vars();
            unsafe {
                env::set_var(KEYCLOAK_REALM_URL, "http://example.com/realms/test-realm");
            }

            let merged = AppConfig::default().merge_with_env();

            assert_eq!(
                merged.keycloak_config.url,
                "http://example.com/realms/test-realm"
            );

            clear_postgres_env_vars();
            clear_redis_env_vars();
        });
    }

    #[test]
    fn default_app_config_contains_keycloak_default_url() {
        let config = AppConfig::default();

        assert_eq!(config.keycloak_config.url, "http://localhost:9999/realms/gateway-realm");
    }

    #[test]
    fn save_config_writes_modified_keycloak_value_to_file() {
        let dir = tempdir().expect("failed to create temp dir");
        let manager = ConfigManager::new(Some(dir.path().to_path_buf()));

        let mut config = AppConfig::default();
        config.keycloak_config.url = "https://auth.example.com/realms/custom".to_string();

        manager.save_config(&config).expect("save_config failed");

        let contents = fs::read_to_string(manager.get_config_file_path())
            .expect("read config failed");
        let parsed: AppConfig = toml::from_str(&contents).expect("parse config failed");

        assert_eq!(parsed.keycloak_config.url, "https://auth.example.com/realms/custom");
    }

    #[test]
    fn save_config_roundtrip_preserves_keycloak_values() {
        let dir = tempdir().expect("failed to create temp dir");
        let manager = ConfigManager::new(Some(dir.path().to_path_buf()));

        let mut expected = AppConfig::default();
        expected.keycloak_config.url = "https://auth.example.com/realms/custom".to_string();

        manager.save_config(&expected).expect("save_config failed");
        let actual = manager.load_or_create().expect("load_or_create failed");

        assert_eq!(actual, expected);
    }
}
