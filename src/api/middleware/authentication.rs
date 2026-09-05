use std::{future::Future, pin::Pin};

use anyhow::Result as AnyhowResult;
use axum::{
    Extension,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use hyper::{StatusCode, header};

use crate::{
    app::state::AppState,
    service::keycloak::{KeycloakMetadata, KeycloakService, VerifiedPrincipal},
};

pub trait AuthService: Send + Sync + 'static {
    fn discover<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = AnyhowResult<KeycloakMetadata>> + Send + 'a>>;

    fn validate_bearer_token<'a>(
        &'a self,
        bearer_token: &'a str,
    ) -> Pin<Box<dyn Future<Output = AnyhowResult<VerifiedPrincipal>> + Send + 'a>>;
}

impl AuthService for KeycloakService {
    fn discover<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = AnyhowResult<KeycloakMetadata>> + Send + 'a>> {
        Box::pin(async move { KeycloakService::discover(self).await })
    }

    fn validate_bearer_token<'a>(
        &'a self,
        bearer_token: &'a str,
    ) -> Pin<Box<dyn Future<Output = AnyhowResult<VerifiedPrincipal>> + Send + 'a>> {
        Box::pin(async move { KeycloakService::validate_bearer_token(self, bearer_token).await })
    }
}

/// `AuthContext` holds the verified principal from the incoming token vrification process
#[derive(Clone)]
pub struct AuthContext {
    pub principal: VerifiedPrincipal,
}

impl AuthContext {
    pub fn new(principal: VerifiedPrincipal) -> Self {
        Self { principal }
    }
}

/// `require_valid_keycloak_token` verifies and extracts the incoming token data
/// it's middleware function/handler
pub async fn require_valid_token(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // try to extract the authentication header value, the bearer token
    let bearer_token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.starts_with("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // use the configured auth validator to validate the bearer token and extract the principal
    let principal = state
        .keycloak_service
        .validate_bearer_token(bearer_token)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // add the principal to the request, so it can be used in downstream handlers
    request.extensions_mut().insert(principal.clone());

    Ok(next.run(request).await)
}

/// Require a verified principal to have the Keycloak `ADMIN` role.
pub async fn require_admin_role(
    State(_state): State<AppState>,
    Extension(principal): Extension<VerifiedPrincipal>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let has_admin_role = principal
        .claims
        .realm_access
        .roles
        .iter()
        .any(|role| role.eq_ignore_ascii_case("ADMIN"))
        || principal
            .claims
            .resource_access
            .values()
            .flat_map(|access| access.roles.iter())
            .any(|role| role.eq_ignore_ascii_case("ADMIN"));

    if !has_admin_role {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use anyhow::Result as AnyhowResult;
    use axum::{
        Router,
        body::Body,
        extract::Extension,
        http::{Request, StatusCode},
        middleware,
        routing::get,
    };
    use deadpool_redis::{Config as RedisConfig, Runtime as RedisRuntime};
    use reqwest::Client;
    use sqlx::PgPool;
    use tower::util::ServiceExt;

    use crate::{
        api::middleware::authentication::{AuthService, require_admin_role, require_valid_token},
        app::state::AppState,
        service::keycloak::{
            KeycloakClaims, KeycloakMetadata, RealmAccess, ResourceAccess, VerifiedPrincipal,
        },
    };

    #[derive(Clone)]
    struct MockAuthService {
        principal: Option<VerifiedPrincipal>,
        should_fail: bool,
        failure_message: String,
    }

    impl AuthService for MockAuthService {
        fn discover<'a>(
            &'a self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = AnyhowResult<KeycloakMetadata>> + Send + 'a>,
        > {
            Box::pin(async { Err(anyhow::anyhow!("mock discover not implemented")) })
        }

        fn validate_bearer_token<'a>(
            &'a self,
            bearer_token: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = AnyhowResult<VerifiedPrincipal>> + Send + 'a>,
        > {
            let token = bearer_token.to_string();
            let principal = self.principal.clone();
            let should_fail = self.should_fail;
            let failure_message = self.failure_message.clone();

            Box::pin(async move {
                if !token.starts_with("Bearer ") {
                    return Err(anyhow::anyhow!("invalid bearer header"));
                }
                if should_fail {
                    return Err(anyhow::anyhow!(failure_message));
                }
                principal.ok_or_else(|| anyhow::anyhow!("missing mocked principal"))
            })
        }
    }

    fn test_principal(subject: &str) -> VerifiedPrincipal {
        VerifiedPrincipal {
            subject: subject.to_string(),
            claims: KeycloakClaims {
                sub: subject.to_string(),
                iss: "http://localhost:8080/realms/test".to_string(),
                aud: Some(vec!["pulse-gate".to_string()]),
                exp: 9_999_999_999usize,
                nbf: None,
                iat: None,
                realm_access: RealmAccess::default(),
                resource_access: HashMap::new(),
            },
        }
    }

    fn admin_principal(subject: &str) -> VerifiedPrincipal {
        let mut principal = test_principal(subject);
        principal.claims.realm_access.roles = vec!["ADMIN".to_string()];
        principal
    }

    fn user_principal(subject: &str) -> VerifiedPrincipal {
        let mut principal = test_principal(subject);
        principal.claims.realm_access.roles = vec!["viewer".to_string()];
        principal
    }

    fn test_app_state(auth: Arc<dyn AuthService>) -> AppState {
        let redis_pool = RedisConfig::from_url("redis://127.0.0.1:6379")
            .create_pool(Some(RedisRuntime::Tokio1))
            .expect("failed to create redis pool");
        let postgres_pool =
            PgPool::connect_lazy("postgres://postgres:postgres@127.0.0.1:5432/postgres")
                .expect("failed to create postgres pool");
        let service_routes = Arc::new(dashmap::DashMap::new());

        let route_target_repo = Arc::new(crate::model::route_target::RouteTargetRepo::new(
            postgres_pool.clone(),
        ));

        AppState::new(
            redis_pool,
            postgres_pool,
            auth,
            route_target_repo,
            service_routes,
            Client::new(),
            "test-service",
        )
    }

    #[tokio::test]
    async fn middleware_rejects_missing_authorization_header() {
        let state = test_app_state(Arc::new(MockAuthService {
            principal: None,
            should_fail: true,
            failure_message: "should not be called".to_string(),
        }));
        let app = Router::new()
            .route("/secure", get(|| async { StatusCode::OK }))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_valid_token,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/secure")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn middleware_rejects_invalid_token() {
        let state = test_app_state(Arc::new(MockAuthService {
            principal: None,
            should_fail: true,
            failure_message: "token rejected".to_string(),
        }));
        let app = Router::new()
            .route("/secure", get(|| async { StatusCode::OK }))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_valid_token,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/secure")
            .header("authorization", "Bearer invalid-token")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn middleware_accepts_valid_token_and_sets_principal_for_downstream_handler() {
        let principal = test_principal("user-42");
        let state = test_app_state(Arc::new(MockAuthService {
            principal: Some(principal.clone()),
            should_fail: false,
            failure_message: String::new(),
        }));
        let app = Router::new()
            .route(
                "/secure",
                get(
                    |Extension(principal): Extension<VerifiedPrincipal>| async move {
                        principal.subject
                    },
                ),
            )
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_valid_token,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/secure")
            .header("authorization", "Bearer valid-token")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_middleware_allows_admin_principal() {
        let principal = admin_principal("admin-user");
        let state = test_app_state(Arc::new(MockAuthService {
            principal: Some(principal),
            should_fail: false,
            failure_message: String::new(),
        }));
        let app = Router::new()
            .route("/secure", get(|| async { StatusCode::OK }))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_admin_role,
            ))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_valid_token,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/secure")
            .header("authorization", "Bearer valid-token")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_middleware_rejects_non_admin_principal() {
        let principal = user_principal("regular-user");
        let state = test_app_state(Arc::new(MockAuthService {
            principal: Some(principal),
            should_fail: false,
            failure_message: String::new(),
        }));
        let app = Router::new()
            .route("/secure", get(|| async { StatusCode::OK }))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_admin_role,
            ))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_valid_token,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/secure")
            .header("authorization", "Bearer valid-token")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_middleware_accepts_admin_from_resource_access() {
        let mut principal = test_principal("resource-admin");
        principal.claims.resource_access.insert(
            "client-app".to_string(),
            ResourceAccess {
                roles: vec!["ADMIN".to_string()],
            },
        );

        let state = test_app_state(Arc::new(MockAuthService {
            principal: Some(principal),
            should_fail: false,
            failure_message: String::new(),
        }));
        let app = Router::new()
            .route("/secure", get(|| async { StatusCode::OK }))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_admin_role,
            ))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_valid_token,
            ))
            .with_state(state);

        let request = Request::builder()
            .uri("/secure")
            .header("authorization", "Bearer valid-token")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
