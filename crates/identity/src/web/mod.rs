//! The identity endpoints.
//!
//! Java counterpart: `identity/internal/web/JwksController.java` and `AuthController.java`.

use axum::Json;
use axum::extract::State;
use axum::http::Method;
use ledgerly_shared::web::{Body, Routes};
use serde::Serialize;

use crate::access_token::{AccessTokens, PublicJwk};

/// `GET /.well-known/jwks.json`. The spec's `servers: [{url: /}]` puts it outside `/api/v1`.
const JWKS: &str = "/.well-known/jwks.json";

/// The endpoints of Ledgerly's own token issuing (Java's `@OwnTokenIssuing` beans).
pub(crate) fn routes(tokens: &AccessTokens) -> Routes {
    Routes::new().route(Method::GET, JWKS, Body::Ignored, jwks, tokens.clone())
}

/// Java: `JwkSet(List.of(jwk))`.
#[derive(Debug, Serialize)]
struct JwkSet {
    keys: [PublicJwk; 1],
}

/// The public half of the signing key. Java: `JwksController.getJwks`.
async fn jwks(State(tokens): State<AccessTokens>) -> Json<JwkSet> {
    Json(JwkSet {
        keys: [tokens.keys().public_jwk()],
    })
}

#[cfg(test)]
mod tests;
