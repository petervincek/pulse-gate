use serde::{Deserialize, Serialize};

// KEYCLOAK env variables
pub const KEYCLOAK_REALM_URL: &str = "KEYCLOAK_REALM_URL";
pub const KEYCLOAK_JWKS_REFRESH_LOOP_INTERVAL_SEC: &str = "KEYCLOAK_JWKS_REFRESH_LOOP_INTERVAL_SEC";

/// `KeycloakConfig` contains keycloak or openid server specific configuration
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct KeycloakConfig {
    pub url: String,
    pub jwks_refresh_loop_interval_sec: u64,
}

/// Provides default starting values for keycloak (openid server) configuration
impl Default for KeycloakConfig {
    fn default() -> Self {
        Self {
            url: String::from("http://localhost:9999/realms/gateway-realm"),
            jwks_refresh_loop_interval_sec: 5 * 60,
        }
    }
}
