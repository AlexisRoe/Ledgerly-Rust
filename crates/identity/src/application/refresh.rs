//! Rotating the refresh token, and logging out. Java: `identity/internal/application/Refresh.java` and
//! `Logout.java`.

use std::sync::Arc;

use chrono::TimeDelta;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::text::is_blank;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;

use super::{Session, UseCaseError};
use crate::access_token::AccessTokens;
use crate::events::IdentityEvent;
use crate::opaque_token::{self, TokenHash};
use crate::persistence;

/// Exchanges a refresh token for a new access token and a new refresh token in the same family.
#[derive(Debug, Clone)]
pub(crate) struct Refresh {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    events: Arc<dyn Publisher>,
    tokens: AccessTokens,
    refresh_ttl: TimeDelta,
}

impl Refresh {
    pub(crate) const fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        events: Arc<dyn Publisher>,
        tokens: AccessTokens,
        refresh_ttl: TimeDelta,
    ) -> Self {
        Self {
            pool,
            clock,
            events,
            tokens,
            refresh_ttl,
        }
    }

    /// Rotates the token. Its row is locked, so two refreshes with one token are serialised and the second
    /// finds it already replaced.
    ///
    /// A token that was already exchanged is a stolen or replayed one: the whole family is revoked, and that
    /// revocation is committed although the answer is 401 (Java's `noRollbackFor = Unauthenticated.class`).
    /// Each successor lives `refresh-token-ttl` from its own creation: the session slides, without an absolute
    /// cap, as in Java.
    pub(crate) async fn execute(&self, token: Option<&SecretString>) -> Result<Session, UseCaseError> {
        let token = token
            .filter(|token| !is_blank(token.expose_secret()))
            .ok_or(UseCaseError::Unauthenticated)?;
        let now = self.clock.now();
        let mut transaction = self.pool.begin().await?;
        let current = persistence::find_refresh_token_for_update(&mut transaction, &TokenHash::of(token))
            .await?
            .ok_or(UseCaseError::Unauthenticated)?;
        if current.replaced_by.is_some() {
            persistence::revoke_family(&mut transaction, current.family, now).await?;
            let event = IdentityEvent::refresh_token_reuse_detected(current.user, current.family, now);
            self.events.publish(&mut transaction, &event).await?;
            transaction.commit().await?;
            tracing::warn!(
                "Refresh token reuse detected; session family {} revoked",
                current.family
            );
            return Err(UseCaseError::Unauthenticated);
        }
        if current.revoked_at.is_some() || current.is_expired(now) {
            return Err(UseCaseError::Unauthenticated);
        }
        let next = opaque_token::generate().map_err(|_| UseCaseError::Crypto("no random bytes"))?;
        let expires_at = now
            .checked_add_signed(self.refresh_ttl)
            .ok_or(UseCaseError::Crypto("refresh lifetime out of range"))?;
        let successor = persistence::insert_refresh_token(
            &mut transaction,
            current.family,
            current.user,
            &TokenHash::of(&next),
            now,
            expires_at,
        )
        .await?;
        persistence::mark_replaced(&mut transaction, current.id, successor).await?;
        let issued = self
            .tokens
            .issue(current.user)
            .map_err(|_| UseCaseError::Crypto("the access token could not be signed"))?;
        transaction.commit().await?;
        Ok(Session {
            access_token: issued.token,
            expires_in_seconds: issued.expires_in_seconds,
            refresh_token: next,
            refresh_ttl: self.refresh_ttl,
        })
    }
}

/// Ends a session by revoking its refresh-token family.
#[derive(Debug, Clone)]
pub(crate) struct Logout {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}

impl Logout {
    pub(crate) const fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }

    /// Unknown or missing tokens are fine: logging out is idempotent.
    pub(crate) async fn execute(&self, token: Option<&SecretString>) -> Result<(), UseCaseError> {
        let Some(token) = token.filter(|token| !is_blank(token.expose_secret())) else {
            return Ok(());
        };
        let mut transaction = self.pool.begin().await?;
        if let Some(found) = persistence::find_refresh_token_for_update(&mut transaction, &TokenHash::of(token)).await?
        {
            persistence::revoke_family(&mut transaction, found.family, self.clock.now()).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}
