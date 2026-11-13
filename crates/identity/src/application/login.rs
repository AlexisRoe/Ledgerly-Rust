//! Logging in. Java: `identity/internal/application/Login.java` and `Session.java`.

use std::fmt;
use std::sync::Arc;

use chrono::TimeDelta;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::ratelimit::{RateLimit, RateLimiter};
use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;

use super::{UseCaseError, normalise_email, password_matches};
use crate::access_token::AccessTokens;
use crate::opaque_token::{self, TokenHash};
use crate::persistence;

/// The limiter scope of failed logins. Java: `Login.LIMIT_SCOPE`.
const LIMIT_SCOPE: &str = "login";

/// The result of logging in or refreshing: an access token for the body, a refresh token for the cookie.
pub(crate) struct Session {
    pub access_token: SecretString,
    pub expires_in_seconds: i64,
    pub refresh_token: SecretString,
    pub refresh_ttl: TimeDelta,
}

/// Java: `Session.toString()` is `Session[***]`.
impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Session[***]")
    }
}

/// Checks email and password and starts a session.
#[derive(Debug, Clone)]
pub(crate) struct Login {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    tokens: AccessTokens,
    limiter: RateLimiter,
    failures: RateLimit,
    refresh_ttl: TimeDelta,
}

impl Login {
    pub(crate) const fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        tokens: AccessTokens,
        limiter: RateLimiter,
        failures: RateLimit,
        refresh_ttl: TimeDelta,
    ) -> Self {
        Self {
            pool,
            clock,
            tokens,
            limiter,
            failures,
            refresh_ttl,
        }
    }

    /// Unknown email, wrong password and a locked address are all the same [`UseCaseError::Unauthenticated`].
    ///
    /// After `max-failed-logins` failures for one email the login is refused, even with the right password,
    /// until the lock period is over. Failures are counted per email address whether or not it belongs to an
    /// account, so an address registered later starts locked (Java's learning log, M10).
    pub(crate) async fn execute(&self, email: &str, password: SecretString) -> Result<Session, UseCaseError> {
        let now = self.clock.now();
        let email = normalise_email(email);
        if self.limiter.is_exhausted(LIMIT_SCOPE, &email, self.failures) {
            return Err(UseCaseError::Unauthenticated);
        }
        let mut connection = self.pool.acquire().await?;
        let user = persistence::find_user_by_email(&mut connection, &email).await?;
        // An unknown email is checked against a dummy hash, and an account managed by an identity provider has
        // no password: both take the time of a wrong password and get its answer.
        let stored = user.as_ref().and_then(|user| user.password_hash.clone());
        let matched = password_matches(password, stored).await;
        let Some(user) = user.filter(|_| matched) else {
            let _ = self.limiter.try_consume(LIMIT_SCOPE, &email, self.failures);
            return Err(UseCaseError::Unauthenticated);
        };
        self.limiter.reset(LIMIT_SCOPE, &email, self.failures);
        let refresh_token = opaque_token::generate().map_err(|_| UseCaseError::Crypto("no random bytes"))?;
        let expires_at = now
            .checked_add_signed(self.refresh_ttl)
            .ok_or(UseCaseError::Crypto("refresh lifetime out of range"))?;
        // A new session is a new family, with a random v4 id as in Java.
        persistence::insert_refresh_token(
            &mut connection,
            Uuid::new_v4(),
            user.id,
            &TokenHash::of(&refresh_token),
            now,
            expires_at,
        )
        .await?;
        let issued = self
            .tokens
            .issue(user.id)
            .map_err(|_| UseCaseError::Crypto("the access token could not be signed"))?;
        Ok(Session {
            access_token: issued.token,
            expires_in_seconds: issued.expires_in_seconds,
            refresh_token,
            refresh_ttl: self.refresh_ttl,
        })
    }
}
