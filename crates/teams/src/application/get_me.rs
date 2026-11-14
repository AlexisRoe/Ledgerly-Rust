//! `GET /me`: the caller's account, their teams and their open invitations. Java:
//! `teams/internal/application/GetMe.java`, served by teams so that identity never depends on teams (Java's
//! decision D5).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use ledgerly_shared::clock::Clock;
use ledgerly_shared::id::{InvitationId, TeamId, UserId};
use ledgerly_shared::role::Role;
use sqlx::PgPool;

use super::UseCaseError;
use crate::domain::{InvitationStatus, normalise_email};
use crate::persistence::{self, TeamRole};

/// An invitation waiting for the caller's answer: not expired, not answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingInvitation {
    pub id: InvitationId,
    pub team: TeamId,
    pub team_name: String,
    pub role: Role,
    pub expires_at: DateTime<Utc>,
}

/// The caller's account, their teams and their open invitations. `verified` is the email's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Me {
    pub id: UserId,
    pub email: String,
    pub verified: bool,
    pub teams: Vec<TeamRole>,
    pub invitations: Vec<PendingInvitation>,
}

#[derive(Debug, Clone)]
pub(crate) struct GetMe {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}

impl GetMe {
    pub(crate) const fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }

    /// An account that no longer exists is [`UseCaseError::Unauthenticated`]. Invitations are matched by
    /// email, so only an address the user has proven to own may see them.
    pub(crate) async fn execute(&self, caller: UserId) -> Result<Me, UseCaseError> {
        // Java: `@Transactional(readOnly = true)`.
        let mut transaction = self.pool.begin().await?;
        let user = ledgerly_identity::find_user(&mut transaction, caller)
            .await?
            .ok_or(UseCaseError::Unauthenticated)?;
        let teams = persistence::teams_of(&mut transaction, caller).await?;
        let mut invitations = Vec::new();
        if user.verified {
            let now = self.clock.now();
            invitations = persistence::pending_for_email(&mut transaction, &normalise_email(&user.email))
                .await?
                .into_iter()
                .filter(|open| open.invitation.status(now) == InvitationStatus::Pending)
                .map(|open| PendingInvitation {
                    id: open.invitation.id,
                    team: open.invitation.team,
                    team_name: open.team_name,
                    role: open.invitation.role,
                    expires_at: open.invitation.expires_at,
                })
                .collect();
        }
        transaction.commit().await?;
        Ok(Me {
            id: user.id,
            email: user.email,
            verified: user.verified,
            teams,
            invitations,
        })
    }
}
