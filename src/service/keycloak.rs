use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, anyhow};
use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use openidconnect::{
    IssuerUrl, JsonWebKey, JsonWebKeySetUrl,
    core::{CoreJsonWebKeySet, CoreProviderMetadata},
    reqwest::Client,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::RwLock,
    time::{Duration, MissedTickBehavior, interval},
};
use tracing::{debug, info, warn};

use crate::core::config::common::AppConfig;

#[derive(Debug, Deserialize, Serialize)]
struct JwkEntry {
    n: Option<String>,
    e: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RealmAccess {
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResourceAccess {
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Audience {
    Single(String),
    Multiple(Vec<String>),
}

impl Audience {
    pub fn contains(&self, expected: &str) -> bool {
        match self {
            Audience::Single(value) => value == expected,
            Audience::Multiple(values) => values.iter().any(|value| value == expected),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeycloakClaims {
    pub sub: String,
    pub iss: String,
    pub aud: Option<Audience>,
    pub exp: usize,

    #[serde(default)]
    pub nbf: Option<usize>,

    #[serde(default)]
    pub iat: Option<usize>,

    #[serde(default)]
    pub realm_access: RealmAccess,

    #[serde(default)]
    pub resource_access: HashMap<String, ResourceAccess>,
}

#[derive(Debug, Clone)]
pub struct VerifiedPrincipal {
    pub claims: KeycloakClaims,
    pub subject: String,
}

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
    jwks_refresk_loop_interval_sec: u64,
    expected_audience: Option<String>,
    jwks_cache: Arc<RwLock<Option<CoreJsonWebKeySet>>>,
}

impl KeycloakService {
    pub fn new(http_client: Client, app_config: &AppConfig) -> Self {
        Self {
            http_client,
            realm_url: app_config.keycloak_config.url.clone(),
            jwks_refresk_loop_interval_sec: app_config
                .keycloak_config
                .jwks_refresh_loop_interval_sec,
            expected_audience: None,
            jwks_cache: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn discover(&self) -> Result<KeycloakMetadata> {
        let issuer_url = IssuerUrl::new(self.realm_url.clone())?;
        debug!(issuer = %self.realm_url, "Discovering Keycloak provider metadata and JWKS");

        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url.clone(), &self.http_client).await?;

        let jwks_uri = provider_metadata.jwks_uri().clone();
        debug!(jwks_uri = %jwks_uri, "Keycloak provider metadata discovered, fetching JWKS");

        let jwks = CoreJsonWebKeySet::fetch_async(&jwks_uri, &self.http_client).await?;
        debug!(keys_count = jwks.keys().len(), "JWKS fetched successfully");

        Ok(KeycloakMetadata {
            issuer: issuer_url,
            jwks_uri,
            provider_metadata,
            jwks,
        })
    }

    pub async fn get_jwks(&self) -> Result<CoreJsonWebKeySet> {
        if let Some(cached) = self.jwks_cache.read().await.clone() {
            debug!(keys_count = cached.keys().len(), "Using cached JWKS");
            return Ok(cached);
        }

        debug!("JWKS cache miss; fetching fresh JWKS");
        let discovered = self.discover().await?;
        let jwks = discovered.jwks.clone();

        *self.jwks_cache.write().await = Some(jwks.clone());
        debug!(keys_count = jwks.keys().len(), "Fresh JWKS stored in cache");
        Ok(jwks)
    }

    pub async fn invalidate_jwks_cache(&self) {
        debug!("Invalidating JWKS cache");
        *self.jwks_cache.write().await = None;
    }

    pub fn start_jwks_refresh_loop(self: Arc<Self>) {
        // this method consumes itself as the service itself is moved to another thread
        // where the actual refresh logic will be running
        debug!(
            interval_seconds = self.jwks_refresk_loop_interval_sec,
            "Starting background JWKS refresh loop"
        );

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(self.jwks_refresk_loop_interval_sec));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                ticker.tick().await;

                match self.discover().await {
                    Ok(metadata) => {
                        let refreshed_jwks = metadata.jwks.clone();
                        *self.jwks_cache.write().await = Some(refreshed_jwks.clone());
                        info!(
                            keys_count = refreshed_jwks.keys().len(),
                            "JWKS refreshed successfully"
                        );
                    }
                    Err(err) => {
                        warn!(error = %err, "JWKS refresh failed, retaining cached keys");
                    }
                }
            }
        });
    }

    pub fn set_expected_audience(&mut self, audience: impl Into<String>) {
        self.expected_audience = Some(audience.into());
    }

    pub async fn validate_bearer_token(&self, bearer_token: &str) -> Result<VerifiedPrincipal> {
        let token = bearer_token.strip_prefix("Bearer ").ok_or_else(|| {
            warn!(raw_token = %bearer_token, "Authorization header missing 'Bearer ' prefix");
            anyhow!("missing Bearer prefix")
        })?;

        debug!(
            token_prefix = "Bearer",
            token_length = token.len(),
            "Attempting JWT validation"
        );

        let header =
            jsonwebtoken::decode_header(token).map_err(|e| anyhow!("invalid JWT header: {e}"))?;

        let kid = header.kid.clone().ok_or_else(|| {
            warn!("JWT header does not contain a 'kid' claim");
            anyhow!("missing JWT kid")
        })?;

        debug!(kid = %kid, "JWT header parsed successfully");

        let jwks = self.get_jwks().await?;

        let matching_jwk = jwks
            .keys()
            .iter()
            .find(|k| k.key_id().map(|id| id.to_string()) == Some(kid.clone()));

        let matching_jwk = match matching_jwk {
            Some(jwk) => {
                debug!(kid = %kid, "Matched JWK in Keycloak JWKS");
                jwk
            }
            None => {
                warn!(kid = %kid, keys_count = jwks.keys().len(), "No matching JWK found for token kid");
                return Err(anyhow!("no matching JWK found for kid: {kid}"));
            }
        };

        let matching_jwk_json = serde_json::to_value(matching_jwk)
            .map_err(|e| anyhow!("could not serialize JWK for verification: {e}"))?;

        let jwk: JwkEntry = serde_json::from_value(matching_jwk_json)
            .map_err(|e| anyhow!("could not deserialize JWK fields for verification: {e}"))?;

        let n = jwk.n.ok_or_else(|| {
            warn!(kid = %kid, "JWK is missing modulus (n)");
            anyhow!("JWK missing modulus")
        })?;
        let e = jwk.e.ok_or_else(|| {
            warn!(kid = %kid, "JWK is missing exponent (e)");
            anyhow!("JWK missing exponent")
        })?;

        let decoding_key = DecodingKey::from_rsa_components(&n, &e)?;
        let mut validation = Validation::new(Algorithm::RS256);

        if self.expected_audience.is_none() {
            validation.validate_aud = false;
        } else {
            validation.set_audience(&[self.expected_audience.as_deref().unwrap()]);
        }

        let decoded = jsonwebtoken::decode::<KeycloakClaims>(token, &decoding_key, &validation)
            .map_err(|e| {
                warn!(kid = %kid, error = %e, "JWT signature or validation failed");
                anyhow!("JWT verification failed: {e}")
            })?;

        debug!(
            sub = %decoded.claims.sub,
            iss = %decoded.claims.iss,
            exp = decoded.claims.exp,
            audience = ?decoded.claims.aud,
            "JWT decoded successfully; validating claims"
        );

        self.validate_claims(&decoded.claims)?;

        Ok(VerifiedPrincipal {
            subject: decoded.claims.sub.clone(),
            claims: decoded.claims,
        })
    }

    fn validate_claims(&self, claims: &KeycloakClaims) -> Result<()> {
        let now = Utc::now().timestamp() as usize;
        let clock_skew_seconds = 60usize;

        debug!(
            sub = %claims.sub,
            iss = %claims.iss,
            exp = claims.exp,
            nbf = claims.nbf,
            iat = claims.iat,
            aud = ?claims.aud,
            now,
            "Validating Keycloak token claims"
        );

        if claims.iss != self.realm_url {
            warn!(expected_issuer = %self.realm_url, actual_issuer = %claims.iss, "Issuer mismatch");
            return Err(anyhow!(
                "issuer mismatch: expected '{}', got '{}'",
                self.realm_url,
                claims.iss
            ));
        }

        if claims.exp <= now {
            warn!(exp = claims.exp, now, "Token expired");
            return Err(anyhow!(
                "token expired: exp={} is not in the future (now={})",
                claims.exp,
                now
            ));
        }

        if let Some(nbf) = claims.nbf
            && now + clock_skew_seconds < nbf
        {
            warn!(
                nbf = nbf,
                now,
                skew = clock_skew_seconds,
                "Token not valid yet"
            );
            return Err(anyhow!(
                "token not valid yet: nbf={} is in the future (now={})",
                nbf,
                now
            ));
        }

        if let Some(iat) = claims.iat {
            if iat > now + clock_skew_seconds {
                warn!(
                    iat = iat,
                    now,
                    skew = clock_skew_seconds,
                    "Token issued in the future"
                );
                return Err(anyhow!(
                    "token issued in the future: iat={} exceeds now={} with skew {}",
                    iat,
                    now,
                    clock_skew_seconds
                ));
            }

            if iat > claims.exp {
                warn!(iat = iat, exp = claims.exp, "iat is after exp");
                return Err(anyhow!(
                    "token issue time is after expiry: iat={} exp={}",
                    iat,
                    claims.exp
                ));
            }
        }

        if let Some(expected_audience) = &self.expected_audience {
            let has_expected_audience = claims
                .aud
                .as_ref()
                .map(|audiences| audiences.contains(expected_audience))
                .unwrap_or(false);

            if !has_expected_audience {
                warn!(expected_audience = %expected_audience, actual_audience = ?claims.aud, "Audience mismatch");
                return Err(anyhow!(
                    "audience mismatch: expected '{}' in token audiences",
                    expected_audience
                ));
            }
        }

        debug!(sub = %claims.sub, "Keycloak token claims validation passed");
        Ok(())
    }
}
