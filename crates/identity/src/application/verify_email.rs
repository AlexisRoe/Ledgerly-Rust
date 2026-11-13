//! Confirming an email address. Java: `identity/internal/application/VerifyEmail.java`.

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::Publisher;
use secrecy::SecretString;
use sqlx::PgPool;

use super::UseCaseError;
use crate::events::IdentityEvent;
use crate::opaque_token::TokenHash;
use crate::persistence;

/// Confirms an email address with the single-use token from the verification message.
#[derive(Debug, Clone)]
pub(crate) struct VerifyEmail {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    events: Arc<dyn Publisher>,
}

impl VerifyEmail {
    pub(crate) const fn new(pool: PgPool, clock: Arc<dyn Clock>, events: Arc<dyn Publisher>) -> Self {
        Self { pool, clock, events }
    }

    /// Unknown, used and expired tokens are all the same 400 on the field `token`.
    pub(crate) async fn execute(&self, token: &SecretString) -> Result<(), UseCaseError> {
        let now = self.clock.now();
        let mut transaction = self.pool.begin().await?;
        let verification = persistence::find_verification_for_update(&mut transaction, &TokenHash::of(token))
            .await?
            .filter(|verification| verification.is_usable(now))
            .ok_or(UseCaseError::Invalid {
                field: "token",
                message: "Invalid or expired token",
            })?;
        persistence::mark_verification_used(&mut transaction, verification.id, now).await?;
        persistence::mark_verified(&mut transaction, verification.user).await?;
        self.events
            .publish(&mut transaction, &IdentityEvent::user_verified(verification.user, now))
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}
