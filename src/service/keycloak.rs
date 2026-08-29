use anyhow::Result;
use openidconnect::{
    IssuerUrl, JsonWebKeySetUrl,
    core::{CoreJsonWebKeySet, CoreProviderMetadata},
    reqwest::Client,
};

use crate::core::config::common::AppConfig;

pub struct KeycloakMetadata {
    pub issuer: IssuerUrl,
    pub jwks_uri: JsonWebKeySetUrl,
    pub provider_metadata: CoreProviderMetadata,
    pub jwks: CoreJsonWebKeySet,
}

#[derive(Debug, Clone)]
pub struct KeycloakService {
    http_client: Client,
    realm_url: String,
}

impl KeycloakService {
    pub fn new(http_client: Client, app_config: &AppConfig) -> Self {
        Self {
            http_client,
            realm_url: app_config.keycloak_config.url.clone(),
        }
    }

    pub async fn discover(&self) -> Result<KeycloakMetadata> {
        let issuer_url = IssuerUrl::new(self.realm_url.clone())?;

        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url.clone(), &self.http_client).await?;

        let jwks_uri = provider_metadata.jwks_uri().clone();
        let jwks = CoreJsonWebKeySet::fetch_async(&jwks_uri, &self.http_client).await?;

        Ok(KeycloakMetadata {
            issuer: issuer_url,
            jwks_uri,
            provider_metadata,
            jwks,
        })
    }
}
