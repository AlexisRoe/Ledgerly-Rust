//! The identity endpoints.
//!
//! Java counterpart: `identity/internal/web/AuthController.java` and `JwksController.java`. Responses and
//! refusals are Java's: the same 202 body for every registration, the same 401 for every failed login, the
//! refresh token only in an `HttpOnly` cookie and the access token only in the body, `Cache-Control: no-store`
//! on both.

mod cookie;
mod dto;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ledgerly_shared::web::{Body, Problem, Routes, read_json};
use secrecy::ExposeSecret;
use serde::Serialize;

use crate::access_token::{AccessTokens, PublicJwk};
use crate::application::{Login, Register, Session, UseCaseError, VerifyEmail};
use dto::{Accepted, LoginRequest, RegisterRequest, TokenResponse, VerifyRequest};

/// `GET /.well-known/jwks.json`. The spec's `servers: [{url: /}]` puts it outside `/api/v1`.
const JWKS: &str = "/.well-known/jwks.json";

/// Java: `AuthController`'s answer to every registration.
const REGISTERED: &str = "If this address can be registered, a verification message has been sent.";

/// The use cases behind the endpoints.
#[derive(Debug, Clone)]
pub(crate) struct AuthApi {
    pub tokens: AccessTokens,
    pub register: Register,
    pub verify: VerifyEmail,
    pub login: Login,
}

impl AuthApi {
    /// The endpoints of Ledgerly's own token issuing (Java's `@OwnTokenIssuing` beans).
    pub(crate) fn routes(self) -> Routes {
        Routes::new()
            .route(
                Method::POST,
                "/api/v1/auth/register",
                Body::Json,
                register,
                self.clone(),
            )
            .route(Method::POST, "/api/v1/auth/verify", Body::Json, verify, self.clone())
            .route(Method::POST, "/api/v1/auth/login", Body::Json, login, self.clone())
            .route(Method::GET, JWKS, Body::Ignored, jwks, self.tokens)
    }
}

/// `POST /auth/register`: 202 whether or not the address was already registered.
async fn register(State(api): State<AuthApi>, request: Request) -> Result<Response, Problem> {
    let body: RegisterRequest = read_json(request).await?;
    let (Some(email), Some(password)) = (body.email, body.password) else {
        // Validation has required both.
        return Err(Problem::internal_error());
    };
    api.register.execute(&email, password).await.map_err(|e| failure(&e))?;
    Ok((StatusCode::ACCEPTED, Json(Accepted { message: REGISTERED })).into_response())
}

/// `POST /auth/verify`: 204 when the token was valid.
async fn verify(State(api): State<AuthApi>, request: Request) -> Result<Response, Problem> {
    let body: VerifyRequest = read_json(request).await?;
    let Some(token) = body.token else {
        return Err(Problem::internal_error());
    };
    api.verify.execute(&token).await.map_err(|e| failure(&e))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /auth/login`: the access token in the body, the refresh token in the cookie.
async fn login(State(api): State<AuthApi>, request: Request) -> Result<Response, Problem> {
    let body: LoginRequest = read_json(request).await?;
    let (Some(email), Some(password)) = (body.email, body.password) else {
        return Err(Problem::internal_error());
    };
    let session = api.login.execute(&email, password).await.map_err(|e| failure(&e))?;
    tokens(&session)
}

/// 200 with the access token and the rotated cookie. Java: `AuthController.tokens`.
fn tokens(session: &Session) -> Result<Response, Problem> {
    let cookie = cookie::set_cookie(&session.refresh_token, session.refresh_ttl).ok_or_else(Problem::internal_error)?;
    let body = TokenResponse {
        access_token: session.access_token.expose_secret(),
        token_type: "Bearer",
        expires_in: session.expires_in_seconds,
    };
    let mut response = (StatusCode::OK, Json(body)).into_response();
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// Java: `JwkSet(List.of(jwk))`.
#[derive(Debug, Serialize)]
struct JwkSet {
    keys: [PublicJwk; 1],
}

/// `GET /.well-known/jwks.json`: the public half of the signing key. Java: `JwksController.getJwks`.
async fn jwks(State(tokens): State<AccessTokens>) -> Json<JwkSet> {
    Json(JwkSet {
        keys: [tokens.keys().public_jwk()],
    })
}

/// The problem for a failed use case. A database or crypto failure is answered with a bare 500, as Java's
/// `ProblemExceptionHandler.unexpected`, and logged by kind only: a database message can quote a value, such
/// as the email in a constraint violation.
fn failure(error: &UseCaseError) -> Problem {
    match error {
        UseCaseError::Unauthenticated => Problem::not_authenticated(),
        UseCaseError::Invalid { field, message } => Problem::invalid_request(*field, *message),
        UseCaseError::Database(_) => {
            tracing::error!(error = "database", "Unhandled exception");
            Problem::internal_error()
        }
        UseCaseError::Crypto(what) => {
            tracing::error!(error = what, "Unhandled exception");
            Problem::internal_error()
        }
    }
}

#[cfg(test)]
mod tests;
