//! Just-in-time provisioning for the `keycloak` profile. Java:
//! `identity/internal/application/ExternalIdentityProvisioner.java`.
//!
//! The first time a person's token arrives, their user row is created. Ledgerly never uses the provider's id
//! anywhere else; it uses the id of this row (ADR 0001), so everything outside identity works exactly as before.
//!
//! How a token's person is matched, in order:
//! 1. by the provider's `sub` (the normal case, after the first time);
//! 2. by email, but only when the provider says the email is verified: an existing local account with that
//!    address is linked to the `sub`. A provider that hands out unverified emails would otherwise let anyone
//!    take over an account by registering its address;
//! 3. otherwise a new user is created, without a password.

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::id::UserId;
use sqlx::{PgConnection, PgPool};

use super::normalise_email;
use crate::events::IdentityEvent;
use crate::persistence;

/// Why a token's person has no Ledgerly user.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ProvisioningError {
    /// No email to create the account with, or the email belongs to an account the token may not have: Java's
    /// `InvalidBearerTokenException`, a 401.
    #[error("the token cannot be matched to an account")]
    Refused,
    /// Java lets the exception through: a 500.
    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
    /// The second look collided again. Java's second attempt runs outside its catch, so this is a 500 there too.
    #[error("the account changed under two attempts in a row")]
    Collided,
}

/// What one attempt found.
enum Attempt {
    Done(UserId),
    /// A first request of the same person committed in between: look again.
    Raced,
}

/// Creates or links the Ledgerly user for an identity provider's subject.
#[derive(Debug, Clone)]
pub(crate) struct Provision {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    events: Arc<dyn Publisher>,
}

impl Provision {
    pub(crate) const fn new(pool: PgPool, clock: Arc<dyn Clock>, events: Arc<dyn Publisher>) -> Self {
        Self { pool, clock, events }
    }

    /// Ledgerly's id for the person behind a token. Java: `ExternalIdentityProvisioner.resolve`, which looks
    /// once more when two first requests of the same person collide on the unique email or subject.
    ///
    /// # Errors
    ///
    /// [`ProvisioningError::Refused`] for a new person without an email, or an email that belongs to another
    /// account; [`ProvisioningError::Database`] otherwise.
    pub(crate) async fn resolve(
        &self,
        subject: &str,
        email: Option<&str>,
        email_verified: bool,
    ) -> Result<UserId, ProvisioningError> {
        for _ in 0..2 {
            let mut transaction = self.pool.begin().await?;
            if let Attempt::Done(user) = self.provision(&mut transaction, subject, email, email_verified).await? {
                transaction.commit().await?;
                return Ok(user);
            }
        }
        Err(ProvisioningError::Collided)
    }

    async fn provision(
        &self,
        connection: &mut PgConnection,
        subject: &str,
        email: Option<&str>,
        email_verified: bool,
    ) -> Result<Attempt, ProvisioningError> {
        let now = self.clock.now();
        if let Some(user) = persistence::find_user_by_external_subject(connection, subject).await? {
            // The provider is the authority on verification in this mode; it only ever moves towards "verified".
            if email_verified && !user.verified {
                persistence::mark_verified(connection, user.id).await?;
                self.events
                    .publish(connection, &IdentityEvent::user_verified(user.id, now))
                    .await?;
            }
            return Ok(Attempt::Done(user.id));
        }
        let Some(email) = email.filter(|email| !ledgerly_shared::text::is_blank(email)) else {
            return Err(ProvisioningError::Refused);
        };
        let email = normalise_email(email);
        if let Some(existing) = persistence::find_linked_user_by_email(connection, &email).await? {
            if existing.external_subject.as_deref() == Some(subject) {
                // A parallel first request of the same person committed this row after our look by subject.
                return Ok(Attempt::Done(existing.id));
            }
            if !email_verified || existing.external_subject.is_some() {
                // Not provably the same person, or that account already belongs to another provider identity.
                return Err(ProvisioningError::Refused);
            }
            if !persistence::link_external_subject(connection, existing.id, subject).await? {
                return Ok(Attempt::Raced);
            }
            if !existing.verified {
                persistence::mark_verified(connection, existing.id).await?;
                self.events
                    .publish(connection, &IdentityEvent::user_verified(existing.id, now))
                    .await?;
            }
            return Ok(Attempt::Done(existing.id));
        }
        let user = UserId::new_v7();
        if !persistence::insert_external_user(connection, user, &email, subject, email_verified, now).await? {
            return Ok(Attempt::Raced);
        }
        self.events
            .publish(connection, &IdentityEvent::user_registered(user, now))
            .await?;
        if email_verified {
            self.events
                .publish(connection, &IdentityEvent::user_verified(user, now))
                .await?;
        }
        Ok(Attempt::Done(user))
    }
}

#[cfg(test)]
mod tests;
