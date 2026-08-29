use serde::{Deserialize, Serialize};

// KEYCLOAK env variables
pub const KEYCLOAK_REALM_URL: &str = "KEYCLOAK_REALM_URL";

/// `KeycloakConfig` contains keycloak or openid server specific configuration
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct KeycloakConfig {
    pub url: String,
}

/// Provides default starting values for keycloak (openid server) configuration
impl Default for KeycloakConfig {
    fn default() -> Self {
        Self {
            url: String::from("http://localhost:9999/realms/gateway-realm"),
        }
    }
}
