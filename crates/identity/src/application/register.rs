//! Creating an account. Java: `identity/internal/application/Register.java`.

use std::sync::Arc;

use chrono::TimeDelta;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::id::UserId;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;

use super::{UseCaseError, hash_password, normalise_email};
use crate::events::IdentityEvent;
use crate::opaque_token::{self, TokenHash};
use crate::persistence;

/// Creates an unverified account and issues its email-verification token.
#[derive(Debug, Clone)]
pub(crate) struct Register {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    events: Arc<dyn Publisher>,
    verification_ttl: TimeDelta,
    /// The `local` profile without `test`: the verification link is logged, as Java's
    /// `LocalVerificationLogger` does in place of an email service.
    log_verification_links: bool,
}

impl Register {
    pub(crate) const fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        events: Arc<dyn Publisher>,
        verification_ttl: TimeDelta,
        log_verification_links: bool,
    ) -> Self {
        Self {
            pool,
            clock,
            events,
            verification_ttl,
            log_verification_links,
        }
    }

    /// The same outcome for a new and an already registered email, so addresses cannot be enumerated. The
    /// password is hashed first in both cases, so the two paths take about the same time.
    ///
    /// Java catches the unique-email violation of a concurrent registration but its transaction is then marked
    /// for rollback; here `on conflict do nothing` lets the loser end like any taken address.
    pub(crate) async fn execute(&self, email: &str, password: SecretString) -> Result<(), UseCaseError> {
        let email = normalise_email(email);
        let hash = hash_password(password).await?;
        let now = self.clock.now();
        let mut transaction = self.pool.begin().await?;
        if persistence::find_user_by_email(&mut transaction, &email)
            .await?
            .is_some()
        {
            return Ok(());
        }
        let user = UserId::new_v7();
        if !persistence::insert_user(&mut transaction, user, &email, &hash, now).await? {
            return Ok(());
        }
        let token = opaque_token::generate().map_err(|_| UseCaseError::Crypto("no random bytes"))?;
        let expires_at = now
            .checked_add_signed(self.verification_ttl)
            .ok_or(UseCaseError::Crypto("verification lifetime out of range"))?;
        persistence::insert_verification(&mut transaction, user, &TokenHash::of(&token), expires_at).await?;
        self.events
            .publish(&mut transaction, &IdentityEvent::user_registered(user, now))
            .await?;
        transaction.commit().await?;
        if self.log_verification_links {
            tracing::info!(
                "[local only] verify user {user} with POST /api/v1/auth/verify {{\"token\":\"{}\"}}",
                token.expose_secret()
            );
        }
        Ok(())
    }
}
