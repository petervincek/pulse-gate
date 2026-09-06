use serde::{Deserialize, Serialize};

// SERVER env variables
pub const SERVER_PORT: &str = "SERVER_PORT";

/// `ServerConfig` contains reverse proxy server database specific configuration
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct ServerConfig {
    pub port: u16,
}

/// Provides default starting values for reverse proxy server configuration
impl Default for ServerConfig {
    fn default() -> Self {
        Self { port: 3000 }
    }
}
