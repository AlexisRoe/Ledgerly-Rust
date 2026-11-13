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
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::TimeDelta;
use ledgerly_shared::text::utf16_len;
use ledgerly_shared::web::{Body, Problem, Routes, Violations, read_json};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

use crate::access_token::{AccessTokens, PublicJwk};
use crate::application::{Login, Logout, Refresh, Register, Session, UseCaseError, VerifyEmail};
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
    pub refresh: Refresh,
    pub logout: Logout,
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
            .route(
                Method::POST,
                "/api/v1/auth/refresh",
                Body::Ignored,
                refresh,
                self.clone(),
            )
            .route(Method::POST, "/api/v1/auth/logout", Body::Ignored, logout, self.clone())
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

/// `POST /auth/refresh`: rotates the cookie and returns a new access token.
async fn refresh(State(api): State<AuthApi>, headers: HeaderMap) -> Result<Response, Problem> {
    let token = cookie_session(&headers)?;
    let session = api.refresh.execute(token.as_ref()).await.map_err(|e| failure(&e))?;
    tokens(&session)
}

/// `POST /auth/logout`: 204 and an expired cookie, whether or not a session existed.
async fn logout(State(api): State<AuthApi>, headers: HeaderMap) -> Result<Response, Problem> {
    let token = cookie_session(&headers)?;
    api.logout.execute(token.as_ref()).await.map_err(|e| failure(&e))?;
    let cleared = cookie::set_cookie(&SecretString::from(""), TimeDelta::zero()).ok_or_else(Problem::internal_error)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(header::SET_COOKIE, cleared);
    Ok(response)
}

/// What refresh and logout check before their use case, in Spring's order:
/// 1. the required `X-Requested-With` header is there (`MissingRequestHeaderException`: "header is required");
/// 2. the `refresh_token` cookie has at most 200 characters (the spec's `maxLength`, method validation on the
///    parameter `refreshToken`);
/// 3. the header is exactly `ledgerly`, the CSRF defence of Java's ADR 0004 (`AuthController.requireCustomHeader`).
///
/// Several `X-Requested-With` headers reach Java as one value joined with commas, which is never `ledgerly`.
fn cookie_session(headers: &HeaderMap) -> Result<Option<SecretString>, Problem> {
    let values: Vec<String> = headers
        .get_all("x-requested-with")
        .iter()
        .map(|value| value.as_bytes().iter().map(|&byte| char::from(byte)).collect())
        .collect();
    if values.is_empty() {
        return Err(Problem::invalid_request("X-Requested-With", "header is required"));
    }
    let token = cookie::read(headers);
    let mut violations = Violations::default();
    if token
        .as_ref()
        .is_some_and(|token| utf16_len(token.expose_secret()) > 200)
    {
        violations.add("refreshToken", "size must be between 0 and 200");
    }
    violations.into_result()?;
    if values.join(",") != "ledgerly" {
        return Err(Problem::invalid_request("X-Requested-With", "must be 'ledgerly'"));
    }
    Ok(token)
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
