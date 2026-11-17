//! The identity module: users, passwords, email verification, refresh tokens, JWTs and the JWKS endpoint; with
//! the `keycloak` profile instead, the identity provider's tokens and just-in-time provisioning.
//!
//! Java counterpart: `src/main/java/com/ledgerly/identity/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod access_token;
mod application;
pub mod events;
mod keycloak;
mod opaque_token;
mod password;
mod persistence;
mod strict_json;
mod web;

use std::fmt;
use std::sync::Arc;

use chrono::TimeDelta;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::{AuthConfig, KeycloakConfig, Profiles};
use ledgerly_shared::events::Publisher;
use ledgerly_shared::id::UserId;
use ledgerly_shared::ratelimit::{InvalidRateLimit, RateLimit, RateLimiter};
use ledgerly_shared::web::{Routes, TokenVerifier};
use secrecy::SecretString;
use sqlx::{PgConnection, PgPool};

pub use crate::access_token::{AccessTokens, InvalidToken, KeyError, SigningKeys};
use crate::application::{Login, Logout, Provision, Refresh, Register, VerifyEmail};
use crate::keycloak::ExternalTokens;
use crate::web::AuthApi;

/// Why the identity module cannot start.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("{0}")]
    Keys(#[from] KeyError),
    /// `ledgerly.auth.lock-duration` is zero. Java fails the first login instead.
    #[error("ledgerly.auth.lock-duration must be longer than zero")]
    LockDuration(#[from] InvalidRateLimit),
    /// With `keycloak`: the HTTP client for the identity provider could not be built.
    #[error("cannot set up TLS for the identity provider: {0}")]
    Tls(String),
}

/// A stored password hash for `password`, as registration writes it: Java's `PasswordEncoder` bean, which its
/// demo-data seeder borrows. `None` only if the operating system's random number generator fails. Blocks for
/// tens of milliseconds.
#[must_use]
pub fn password_hash(password: &SecretString) -> Option<String> {
    password::hash(password).ok().map(|hash| hash.as_str().to_owned())
}

/// What other modules may know about a user. Java: `identity/UserSummary.java`.
///
/// The email is only for matching invitations: never store or log it. `Debug` leaves it out.
#[derive(Clone, PartialEq, Eq)]
pub struct UserSummary {
    pub id: UserId,
    pub email: String,
    /// The email address has been confirmed.
    pub verified: bool,
}

impl fmt::Debug for UserSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserSummary")
            .field("id", &self.id)
            .field("verified", &self.verified)
            .finish_non_exhaustive()
    }
}

/// The account's id, email and verified flag, or `None` when it does not exist. Java: `IdentityFacade.find`.
///
/// It takes the caller's connection, so that it reads in the caller's transaction, as Java's does.
///
/// # Errors
///
/// The database error.
pub async fn find_user(connection: &mut PgConnection, user: UserId) -> sqlx::Result<Option<UserSummary>> {
    persistence::find_user_summary(connection, user).await
}

/// The identity module as the app wires it.
#[derive(Debug, Clone)]
pub struct Identity {
    /// `None` with `keycloak`: the identity provider issues the tokens, and Ledgerly signs none (Java's
    /// `@OwnTokenIssuing` beans are not created).
    own: Option<AuthApi>,
    /// What checks the bearer tokens: Ledgerly's own, or the identity provider's.
    verifier: Arc<dyn TokenVerifier>,
}

impl Identity {
    /// Reads or generates the signing key and wires the use cases. Java generates a key only in the `local`
    /// and `test` profiles, and logs verification links only in `local` without `test`. With `keycloak`
    /// (`keycloak` is `Some`), it only prepares the provider's token check, which asks the provider nothing
    /// before the first token.
    ///
    /// # Errors
    ///
    /// The [`IdentityError`] that stops the application from starting.
    pub fn new(
        auth: &AuthConfig,
        keycloak: Option<&KeycloakConfig>,
        profiles: Profiles,
        pool: &PgPool,
        clock: &Arc<dyn Clock>,
        events: &Arc<dyn Publisher>,
        limiter: &RateLimiter,
    ) -> Result<Self, IdentityError> {
        if let Some(keycloak) = keycloak {
            let provision = Provision::new(pool.clone(), clock.clone(), events.clone());
            let external = ExternalTokens::new(&keycloak.issuer_uri, &keycloak.audience, clock.clone(), provision)
                .map_err(|failure| IdentityError::Tls(failure.0))?;
            return Ok(Self {
                own: None,
                verifier: Arc::new(external),
            });
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
        Ok(Self {
            verifier: Arc::new(api.tokens.clone()),
            own: Some(api),
        })
    }

    /// The access tokens Ledgerly signs, if it signs any.
    #[must_use]
    pub fn access_tokens(&self) -> Option<&AccessTokens> {
        self.own.as_ref().map(|api| &api.tokens)
    }

    /// What the authentication layer checks bearer tokens with.
    #[must_use]
    pub fn token_verifier(&self) -> Arc<dyn TokenVerifier> {
        self.verifier.clone()
    }

    /// The identity endpoints: none with `keycloak`.
    #[must_use]
    pub fn routes(&self) -> Routes {
        self.own.clone().map_or_else(Routes::new, AuthApi::routes)
    }
}
