//! The identity module: users, passwords, email verification, refresh tokens, JWTs and the JWKS endpoint.
//!
//! Java counterpart: `src/main/java/com/ledgerly/identity/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod access_token;
mod application;
pub mod events;
mod opaque_token;
mod password;
mod persistence;
mod strict_json;
mod web;

use std::sync::Arc;

use chrono::TimeDelta;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::{AuthConfig, Profiles};
use ledgerly_shared::events::Publisher;
use ledgerly_shared::ratelimit::{InvalidRateLimit, RateLimit, RateLimiter};
use ledgerly_shared::web::Routes;
use secrecy::SecretString;
use sqlx::PgPool;

pub use crate::access_token::{AccessTokens, InvalidToken, KeyError, SigningKeys};
use crate::application::{Login, Logout, Refresh, Register, VerifyEmail};
use crate::web::AuthApi;

/// Why the identity module cannot start.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("{0}")]
    Keys(#[from] KeyError),
    /// `ledgerly.auth.lock-duration` is zero. Java fails the first login instead.
    #[error("ledgerly.auth.lock-duration must be longer than zero")]
    LockDuration(#[from] InvalidRateLimit),
}

/// A stored password hash for `password`, as registration writes it: Java's `PasswordEncoder` bean, which its
/// demo-data seeder borrows. `None` only if the operating system's random number generator fails. Blocks for
/// tens of milliseconds.
#[must_use]
pub fn password_hash(password: &SecretString) -> Option<String> {
    password::hash(password).ok().map(|hash| hash.as_str().to_owned())
}

/// The identity module as the app wires it.
#[derive(Debug, Clone)]
pub struct Identity {
    /// `None` with `keycloak`: the identity provider issues the tokens, and Ledgerly signs none (Java's
    /// `@OwnTokenIssuing` beans are not created).
    own: Option<AuthApi>,
}

impl Identity {
    /// Reads or generates the signing key and wires the use cases. Java generates a key only in the `local`
    /// and `test` profiles, and logs verification links only in `local` without `test`.
    ///
    /// # Errors
    ///
    /// The [`IdentityError`] that stops the application from starting.
    pub fn new(
        auth: &AuthConfig,
        profiles: Profiles,
        pool: &PgPool,
        clock: &Arc<dyn Clock>,
        events: &Arc<dyn Publisher>,
        limiter: &RateLimiter,
    ) -> Result<Self, IdentityError> {
        if profiles.keycloak {
            return Ok(Self { own: None });
        }
        let keys = SigningKeys::from_config(
            auth.jwt_private_key.as_ref(),
            auth.jwt_public_key.as_deref(),
            profiles.local || profiles.test,
        )?;
        let tokens = AccessTokens::new(Arc::new(keys), auth.access_token_ttl, clock.clone());
        let failures = RateLimit::intervally(auth.max_failed_logins, auth.lock_duration)?;
        let lifetime = |ttl| TimeDelta::from_std(ttl).unwrap_or(TimeDelta::MAX);
        let api = AuthApi {
            register: Register::new(
                pool.clone(),
                clock.clone(),
                events.clone(),
                lifetime(auth.verification_ttl),
                profiles.local && !profiles.test,
            ),
            verify: VerifyEmail::new(pool.clone(), clock.clone(), events.clone()),
            login: Login::new(
                pool.clone(),
                clock.clone(),
                tokens.clone(),
                limiter.clone(),
                failures,
                lifetime(auth.refresh_token_ttl),
            ),
            refresh: Refresh::new(
                pool.clone(),
                clock.clone(),
                events.clone(),
                tokens.clone(),
                lifetime(auth.refresh_token_ttl),
            ),
            logout: Logout::new(pool.clone(), clock.clone()),
            tokens,
        };
        Ok(Self { own: Some(api) })
    }

    /// The access tokens Ledgerly signs, if it signs any.
    #[must_use]
    pub fn access_tokens(&self) -> Option<&AccessTokens> {
        self.own.as_ref().map(|api| &api.tokens)
    }

    /// The identity endpoints: none with `keycloak`.
    #[must_use]
    pub fn routes(&self) -> Routes {
        self.own.clone().map_or_else(Routes::new, AuthApi::routes)
    }
}
