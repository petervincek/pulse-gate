use std::{
    collections::HashMap,
    env,
    sync::{Arc, atomic::{AtomicU64, Ordering}},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use axum::Router;
use openidconnect::{reqwest::Client};
use pulse_gate::{app::create_app_router, core::config::{common::ConfigManager, keycloak::KEYCLOAK_REALM_URL, postgres::POSTGRES_URL, redis::REDIS_URL}};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt, core::{IntoContainerPort, Mount, WaitFor}, runners::AsyncRunner,
};
use tokio::{sync::{Mutex, OnceCell}, task::JoinHandle};
use tempfile::TempDir;

// global infrastructure created just once for the whole integration test suite
static INFRA: OnceCell<Arc<TestInfra>> = OnceCell::const_new();

const REDIS_IMAGE: &str = "redis:8-alpine";
const REDIS_PORT: u16 = 6379_u16;
const POSTGRES_IMAGE: &str = "postgres:18-alpine";
const POSTGRES_PORT: u16 = 5432_u16;
const POSTGRES_USER: &str = "pulse_gate";
const POSTGRES_PASSWORD: &str = "pulse_gate_password";
const POSTGRES_DB: &str = "pulse_gate_dev";
const KEYCLOAK_IMAGE: &str = "keycloak/keycloak:26.7";
const KEYCLOAK_PORT: u16 = 8080_u16;
const KEYCLOAK_REALM: &str = "gateway-realm";
const TEST_INFRA_LABEL_KEY: &str = "pulse-gate-test";
const TEST_INFRA_LABEL_VALUE: &str = "true";
const TEST_INFRA_NAME_PREFIX: &str = "pulse-gate-test-";

static TEST_INFRA_INSTANCE_ID: AtomicU64 = AtomicU64::new(0);

fn unique_test_infra_name(prefix: &str) -> String {
    let unique_suffix = TEST_INFRA_INSTANCE_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{prefix}{timestamp_nanos}-{unique_suffix}")
}

pub struct EnvRestore(Vec<(String, Option<String>)>);

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, original) in self.0.drain(..) {
            match original {
                Some(value) => unsafe { env::set_var(key, value) },
                None => unsafe { env::remove_var(key) },
            }
        }
    }
}

pub struct TestServer {
    join_handle: JoinHandle<()>,
    app_url: String,
}

impl TestServer {
    pub fn app_url(&self) -> &str {
        &self.app_url
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.join_handle.abort();
    }
}

pub struct TestInfra {
    _redis: ContainerAsync<GenericImage>,       // redis container
    _postgres: ContainerAsync<GenericImage>,    // postgres container
    _keycloak: ContainerAsync<GenericImage>,    // keycloak container
    pub http_client: Client, // http client pool
    pub env_variables: HashMap<String, String>, // map to hold env variables with connection strings
    pub lock: Mutex<()>, // mutex lock to ensure the infra is available just to one test at the time
}

impl TestInfra {
    pub async fn get_infra() -> Arc<Self> {
        INFRA
            .get_or_init(|| async {
                // prepare the containers
                let redis_container = GenericImage::new(
                    REDIS_IMAGE.split(":").collect::<Vec<&str>>()[0],
                    REDIS_IMAGE.split(":").collect::<Vec<&str>>()[1],
                )
                .with_exposed_port(REDIS_PORT.tcp())
                .with_wait_for(WaitFor::message_on_stdout(
                    "Ready to accept connections tcp",
                ))
                .with_label(TEST_INFRA_LABEL_KEY, TEST_INFRA_LABEL_VALUE)
                .with_container_name(unique_test_infra_name(&format!("{}redis-", TEST_INFRA_NAME_PREFIX)))
                .with_cmd(vec!["redis-server", "--appendonly", "yes"]);

                let postgres_container = GenericImage::new(
                    POSTGRES_IMAGE.split(":").collect::<Vec<&str>>()[0],
                    POSTGRES_IMAGE.split(":").collect::<Vec<&str>>()[1],
                )
                .with_exposed_port(POSTGRES_PORT.tcp())
                .with_wait_for(WaitFor::message_on_stdout(
                    "database system is ready to accept connections",
                ))
                .with_env_var("POSTGRES_USER", POSTGRES_USER)
                .with_env_var("POSTGRES_PASSWORD", POSTGRES_PASSWORD)
                .with_env_var("POSTGRES_DB", POSTGRES_DB)
                .with_label(TEST_INFRA_LABEL_KEY, TEST_INFRA_LABEL_VALUE)
                .with_container_name(unique_test_infra_name(&format!("{}postgres-", TEST_INFRA_NAME_PREFIX)));

                let keyloack_container = GenericImage::new(
                    KEYCLOAK_IMAGE.split(":").collect::<Vec<&str>>()[0],
                    KEYCLOAK_IMAGE.split(":").collect::<Vec<&str>>()[1],
                )
                .with_exposed_port(KEYCLOAK_PORT.tcp())
                .with_wait_for(WaitFor::message_on_stdout("Listening on: http://0.0.0.0:8080"))
                .with_env_var("KEYCLOAK_ADMIN", "admin")
                .with_env_var("KEYCLOAK_ADMIN_PASSWORD", "admin")
                .with_mount(create_keycloak_realm_mount_point())
                .with_label(TEST_INFRA_LABEL_KEY, TEST_INFRA_LABEL_VALUE)
                .with_container_name(unique_test_infra_name(&format!("{}keycloak-", TEST_INFRA_NAME_PREFIX)))
                .with_cmd(vec!["start-dev", "--import-realm"]);

                // start the container in parallel to save time
                let (redis, postgres, keycloak) = tokio::join!(
                    redis_container.start(),
                    postgres_container.start(),
                    keyloack_container.start()
                );
                let redis = redis.expect("failed to start Redis test container");
                let postgres = postgres.expect("failed to start Postgres test container");
                let keycloak = keycloak.expect("failed to start Keycloak test container");

                let redis_host = redis.get_host().await.unwrap();
                let redis_port = redis.get_host_port_ipv4(REDIS_PORT).await.unwrap();
                let redis_url = format!("redis://{redis_host}:{redis_port}");

                let postgres_host = postgres.get_host().await.unwrap();
                let postgres_port = postgres.get_host_port_ipv4(POSTGRES_PORT).await.unwrap();
                let postgres_url = format!("postgres://{POSTGRES_USER}:{POSTGRES_PASSWORD}@{postgres_host}:{postgres_port}/{POSTGRES_DB}");

                let keycloak_host = keycloak.get_host().await.unwrap();
                let keycloak_port = keycloak.get_host_port_ipv4(KEYCLOAK_PORT).await.unwrap();
                let keycloak_realm_url =
                    format!("http://{keycloak_host}:{keycloak_port}/realms/{KEYCLOAK_REALM}");
                
                let mut env_variables: HashMap<String, String> = HashMap::new();
                env_variables.insert(REDIS_URL.to_string(), redis_url);
                env_variables.insert(POSTGRES_URL.to_string(), postgres_url);
                env_variables.insert(KEYCLOAK_REALM_URL.to_string(), keycloak_realm_url);

                Arc::new(TestInfra {
                    _redis: redis,
                    _postgres: postgres,
                    _keycloak: keycloak,
                    http_client: Client::new(),
                    env_variables,
                    lock: Mutex::new(()),
                })
            })
            .await
            .clone()
    }

    /// Helper to spin up an Axum listener for a test case
    pub async fn spawn_app(&self, app_router: Router) -> TestServer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();

        let join_handler = tokio::spawn(async move {
            axum::serve(listener, app_router).await.unwrap();
        });

        TestServer{join_handle: join_handler, app_url: format!("http://{addr}")}
    }

    pub fn export_env_variables(&self, env_vars: Option<&HashMap<String, String>>) -> EnvRestore {
        let env_variables = if let Some(env_variables) = env_vars {
            env_variables
        } else {
            &self.env_variables
        };
        println!("ENV VARIABLES: {:?}", env_variables);

        let original_env = env_variables
            .iter()
            .map(|(key, _)| (key.clone(), env::var(key).ok()))
            .collect();
        let env_restore = EnvRestore(original_env);

        for (key, value) in env_variables {
            unsafe { env::set_var(key, value) };
        }
        env_restore
    }
}

fn create_keycloak_realm_mount_point() -> Mount {
        let realm_export = std::env::current_dir()
            .expect("failed to get current directory")
            .join("tests/fixtures/gateway-realm-export.json")
            .canonicalize()
            .expect("failed to canonicalize keycloak realm export path");

        let keycloak_mount = Mount::bind_mount(
    realm_export
                        .to_str()
                        .expect("failed to convert realm export path to string"),
                    "/opt/keycloak/data/import/realm.json",
                );
                keycloak_mount
}

pub async fn default_app_router(config_dir_tmp: &TempDir) -> Result<Router> {
    let config_manager = ConfigManager::new(Some(config_dir_tmp.path().to_path_buf()));
    let app_config = config_manager.load_or_create()?.merge_with_env();
        
    let app_router = create_app_router(&app_config).await?;
    Ok(app_router)
}
