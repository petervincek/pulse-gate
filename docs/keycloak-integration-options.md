## Keycloak integration options for token verification

For a Rust/Axum API gateway, you can verify Keycloak-issued tokens in a few different ways:

### 1. Verify JWTs locally with Keycloak’s JWKS
- Fetch Keycloak’s public keys from the realm endpoint:
  - `https://<keycloak-host>/realms/<realm>/protocol/openid-connect/certs`
- Verify incoming access tokens locally:
  - Validate signature
  - Validate `iss`, `aud`, `exp`, `nbf`
  - Optionally validate `azp`, roles, scopes, custom claims
- Benefits:
  - Fast, no extra network call per request
  - Works well in gateway/microservice patterns
- Caveats:
  - Need to refresh JWKS on key rotation
  - Must handle `kid` lookup correctly

### 2. Use OIDC discovery plus JWT validation
- Dynamic discovery from:
  - `https://<keycloak-host>/realms/<realm>/.well-known/openid-configuration`
- Retrieve:
  - JWKS URI
  - Issuer
  - Token endpoint
- This is the standard OpenID Connect flow for a Keycloak-backed API.

### 3. Use Keycloak token introspection
- Call Keycloak introspection endpoint:
  - `POST /realms/<realm>/protocol/openid-connect/token/introspect`
- Requires client credentials
- Useful when:
  - You want Keycloak to validate token state centrally
  - You need to check token revocation or active status
- Drawbacks:
  - Extra network call per validation
  - Higher latency than local JWT verification

### 4. Use Keycloak session / userinfo endpoint
- If you already have an access token, you can call:
  - `/realms/<realm>/protocol/openid-connect/userinfo`
- Not typically used for gateway auth, but useful for retrieving user claims

### 5. Use a reverse proxy / API gateway that handles Keycloak auth
- Example: NGINX with OpenID Connect module, Kong, Traefik, OAuth2 proxy
- The proxy authenticates requests and forwards only allowed traffic
- Your Rust service then trusts the proxy headers
- Good for isolating auth logic from app code

### 6. Use a dedicated auth middleware library
For Rust, common approaches include:
- `jsonwebtoken` / `josekit` + manual claim checks
- `openidconnect` crate for discovery and JOSE validation
- `oxide-auth` / `gotham-oauth2` for OAuth/OIDC flows (less common for API-only validation)

### 7. Use Keycloak adapter-style behavior in the gateway
- Validate tokens
- Enforce realm roles or client roles
- Map Keycloak groups/roles to internally meaningful permissions
- Add request context with validated claims

---

## Recommended practical approach

For most Axum API gateway use cases, the best pattern is:

1. Discover Keycloak metadata
2. Fetch JWKS
3. Validate JWT locally
4. Refresh JWKS periodically
5. Enforce claims/roles in middleware

This is the fastest and most scalable option.

---

## Specific token types
- `access_token`
  - Primary token for APIs
  - Usually JWT
- `id_token`
  - Not usually used for API authorization
- `refresh_token`
  - Not used for API requests

---

## Useful Rust crates
- `jsonwebtoken`
- `josekit`
- `openidconnect`
- `axum-extra` / `tower-http` middleware for auth if you want reusable patterns

---

## What to choose for your project
- If you want minimal runtime dependencies and best performance → local JWT validation via JWKS
- If you need immediate revocation and don't mind extra latency → introspection
- If you want auth outside the app entirely → reverse proxy / external gateway

If you want, I can also propose a concrete Axum middleware design and a small Rust implementation sketch for Keycloak JWT validation.

Yes — your understanding is correct.

### What the combined approach means

You would use approach 2 to discover the right endpoints, and then approach 1 to perform the actual JWT signature validation.

### Typical flow

1. Call the discovery endpoint:
   - `https://<keycloak-host>/realms/<realm>/.well-known/openid-configuration`
2. Parse the discovery response to get:
   - `issuer`
   - `jwks_uri`
   - `authorization_endpoint`
   - `token_endpoint`
   - `userinfo_endpoint`
   - `introspection_endpoint`
3. Fetch the JWKS from `jwks_uri`
4. Use the JWKS public keys to verify incoming JWT signatures
5. Validate token claims:
   - `iss` matches the `issuer`
   - `aud` contains your API/client audience
   - `exp` not expired
   - `nbf` if present
   - optionally `azp`, realm roles, client roles, custom claims

### Missing context / extra details

- Keycloak usually signs access tokens with `RS256`, so the JWKS contains RSA public keys.
- Each JWT header includes a `kid`; you use that to select the correct key from JWKS.
- You should cache both:
  - discovery metadata
  - JWKS keys
- Refresh the JWKS periodically or when a token arrives with an unknown `kid`.

### Why this is a good approach

- Discovery gives you a dynamic, standard way to find Keycloak endpoints
- JWKS validation lets you verify tokens locally without a network call per request
- This is the recommended OIDC pattern for API gateways

### Important caveats

- Discovery + JWKS does not validate token revocation or logout state
  - If you need immediate revocation, you must also use introspection or short-lived tokens
- You still need to validate claims after signature verification
- If Keycloak rotates keys, your cached JWKS must refresh

### Example endpoints

Given your `docker-compose` Keycloak:
- discovery: `http://localhost:9999/realms/<realm>/.well-known/openid-configuration`
- JWKS: value of `jwks_uri` from that discovery document
- token introspection: `http://localhost:9999/realms/<realm>/protocol/openid-connect/token/introspect`

### Summary

So yes, the combined flow is:
- Step 1: OIDC discovery
- Step 2: JWKS fetch
- Step 3: JWT signature verification and claim validation

If you want, I can now map that into a concrete Axum + Rust design with the exact Keycloak endpoints and a middleware structure.

## Recommended integration approach

For Rust, the usual and recommended option is:

- use a general OpenID Connect / OIDC client library for discovery and JWKS handling
- then implement token validation logic yourself in middleware

That is the normal pattern, not a Keycloak-specific adapter.

---

## Best Rust library for the combined approach

**`openidconnect`** is the recommended crate:

- handles OIDC discovery
- parses the `.well-known/openid-configuration`
- fetches `jwks_uri`
- supports JWT validation and claim extraction
- works with Keycloak because Keycloak is OIDC compliant

Useful related crates:
- `reqwest` for HTTP fetching if you want lower-level control
- `josekit` or `jsonwebtoken` for raw JWT / JOSE verification
- `axum` / `tower-http` for middleware wrapping

---

## Typical integration style

1. Use OIDC discovery:
   - `https://<host>/realms/<realm>/.well-known/openid-configuration`
2. Read `jwks_uri`, `issuer`, `token_endpoint`, etc.
3. Fetch JWKS from `jwks_uri`
4. Validate the incoming JWT signature locally
5. Verify standard claims:
   - `iss`
   - `aud`
   - `exp`
   - `nbf`
   - optional `azp`, roles, scopes
6. Cache discovery metadata and JWKS
7. Refresh JWKS when unknown `kid` or periodically

---

## Is it usually custom or library-backed?

- In Rust: usually library-backed for discovery and cryptography, but custom integration for middleware and business rules.
- There is no de facto “Keycloak adapter” like in Java.
- So the best practice is:
  - use `openidconnect` for OIDC/Keycloak interaction
  - write your own Axum middleware for request auth

---

## Why this is the usual choice

- Keycloak is standard OIDC, so a generic OIDC client is the right fit
- Local JWT verification is faster than introspection
- You still need custom app logic for:
  - request extraction
  - claim mapping
  - permission/role enforcement
  - caching JWKS/metadata

---

## Bottom line

Yes, the combined approach is correct.

Recommended Rust stack:
- `openidconnect` for discovery + JWKS
- `jsonwebtoken` / `josekit` for JWT verification if desired
- custom Axum middleware for request validation

If you want, I can now sketch the exact middleware design and the crate-level code structure for your gateway.