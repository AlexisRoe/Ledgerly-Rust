//! Inviting, revoking, accepting and declining. Java: `InviteMember`, `RevokeInvitation`, `AcceptInvitation`,
//! `DeclineInvitation` and `InvitationResponses` in `teams/internal/application/`.

use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use ledgerly_identity::UserSummary;
use ledgerly_shared::id::{InvitationId, TeamId, UserId};
use ledgerly_shared::role::Role;
use sqlx::PgConnection;

use super::{Context, UseCaseError, guard};
use crate::domain::{Invitation, InvitationStatus, normalise_email};
use crate::events::{TeamEvent, TeamEventKind};
use crate::persistence;

/// An invitation as the API shows it. `status` may be [`InvitationStatus::Expired`], which is never stored.
/// Java: `InvitationView`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InvitationView {
    pub id: InvitationId,
    pub role: Role,
    pub status: InvitationStatus,
    pub expires_at: DateTime<Utc>,
}

/// Lets an OWNER invite an email address to the team with a role.
#[derive(Debug, Clone)]
pub(crate) struct InviteMember {
    context: Context,
    ttl: TimeDelta,
}

impl InviteMember {
    pub(crate) const fn new(context: Context, ttl: TimeDelta) -> Self {
        Self { context, ttl }
    }

    /// Never looks at whether the address belongs to a registered user or is already a teammate, so the
    /// response cannot be used to find out. Re-inviting replaces the address's pending invitation.
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        caller: UserId,
        email: &str,
        role: Role,
    ) -> Result<InvitationView, UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        guard::lock_and_require_owner(&mut transaction, team, caller).await?;
        let now = self.context.clock.now();
        let email = normalise_email(email);
        if let Some(old) = persistence::find_pending_in_team(&mut transaction, team, &email).await? {
            // Written before the insert below, or the one-pending-per-address index would refuse it. Java
            // publishes no event for the replaced invitation either.
            persistence::set_status(&mut transaction, old.id, InvitationStatus::Revoked).await?;
        }
        let invitation = Invitation {
            id: InvitationId::new_v7(),
            team,
            email,
            role,
            stored_status: InvitationStatus::Pending,
            // Postgres keeps microseconds: the response shows what is stored.
            expires_at: now
                .checked_add_signed(self.ttl)
                .unwrap_or(DateTime::<Utc>::MAX_UTC)
                .trunc_subsecs(6),
        };
        persistence::insert_invitation(&mut transaction, &invitation, caller, now).await?;
        let invited = TeamEventKind::MemberInvited {
            invitation_id: invitation.id,
            role,
        };
        self.context
            .events
            .publish(&mut transaction, &TeamEvent::new(team, caller, now, invited))
            .await?;
        transaction.commit().await?;
        Ok(InvitationView {
            id: invitation.id,
            role,
            status: invitation.status(now),
            expires_at: invitation.expires_at,
        })
    }
}

/// Lets an OWNER withdraw a pending invitation of their team.
#[derive(Debug, Clone)]
pub(crate) struct RevokeInvitation {
    context: Context,
}

impl RevokeInvitation {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// Another team's invitation is simply not found. One that is no longer pending is
    /// [`UseCaseError::InvitationNotPending`].
    pub(crate) async fn execute(&self, team: TeamId, id: InvitationId, caller: UserId) -> Result<(), UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        guard::lock_and_require_owner(&mut transaction, team, caller).await?;
        let now = self.context.clock.now();
        let invitation = persistence::lock_invitation_in_team(&mut transaction, team, id)
            .await?
            .ok_or(UseCaseError::NotFound)?;
        if invitation.status(now) != InvitationStatus::Pending {
            return Err(UseCaseError::InvitationNotPending(
                "Only a pending invitation can be revoked".to_owned(),
            ));
        }
        persistence::set_status(&mut transaction, id, InvitationStatus::Revoked).await?;
        let revoked = TeamEventKind::InvitationRevoked { invitation_id: id };
        self.context
            .events
            .publish(&mut transaction, &TeamEvent::new(team, caller, now, revoked))
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

/// The team joined and the role received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Joined {
    pub team: TeamId,
    pub role: Role,
}

/// Turns an invitation addressed to the caller into a membership.
#[derive(Debug, Clone)]
pub(crate) struct AcceptInvitation {
    context: Context,
}

impl AcceptInvitation {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// Needs, in this order: addressed to the caller (else 404), a verified caller, an invitation that is still
    /// pending and not expired, and a caller who is not a teammate yet.
    pub(crate) async fn execute(&self, id: InvitationId, caller: UserId) -> Result<Joined, UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        let (invitation, user) = addressed(&mut transaction, id, caller, Lock::None).await?;
        if !user.verified {
            return Err(UseCaseError::EmailNotVerified);
        }
        let now = self.context.clock.now();
        require_pending(&invitation, now)?;
        // The same team lock as every membership change, so accepting cannot interleave with removing.
        guard::lock(&mut transaction, invitation.team).await?;
        // Then the invitation's own row, after the team's as in revoking, and checked again: a decline or revoke
        // that committed meanwhile wins (Java does not lock it; decided after M6).
        let invitation = persistence::lock_invitation(&mut transaction, id)
            .await?
            .ok_or(UseCaseError::NotFound)?;
        require_pending(&invitation, now)?;
        if persistence::role_of(&mut transaction, invitation.team, caller)
            .await?
            .is_some()
        {
            return Err(UseCaseError::AlreadyMember);
        }
        persistence::insert_membership(&mut transaction, invitation.team, caller, invitation.role, now).await?;
        persistence::set_status(&mut transaction, id, InvitationStatus::Accepted).await?;
        let accepted = TeamEventKind::InvitationAccepted { invitation_id: id };
        self.context
            .events
            .publish(
                &mut transaction,
                &TeamEvent::new(invitation.team, caller, now, accepted),
            )
            .await?;
        transaction.commit().await?;
        Ok(Joined {
            team: invitation.team,
            role: invitation.role,
        })
    }
}

/// Lets the person an invitation was sent to decline it.
#[derive(Debug, Clone)]
pub(crate) struct DeclineInvitation {
    context: Context,
}

impl DeclineInvitation {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// Not addressed to the caller: 404. Unlike accepting, declining needs no verified email.
    pub(crate) async fn execute(&self, id: InvitationId, caller: UserId) -> Result<(), UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        let (invitation, _) = addressed(&mut transaction, id, caller, Lock::Row).await?;
        let now = self.context.clock.now();
        require_pending(&invitation, now)?;
        persistence::set_status(&mut transaction, id, InvitationStatus::Declined).await?;
        let declined = TeamEventKind::InvitationDeclined { invitation_id: id };
        self.context
            .events
            .publish(
                &mut transaction,
                &TeamEvent::new(invitation.team, caller, now, declined),
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

/// The invitation, if it exists and was sent to the caller's email, with the caller's account. Anyone else gets
/// [`UseCaseError::NotFound`], so an invitation id guessed or leaked to the wrong person reveals nothing. Java:
/// `InvitationResponses.load`.
async fn addressed(
    connection: &mut PgConnection,
    id: InvitationId,
    caller: UserId,
    lock: Lock,
) -> Result<(Invitation, UserSummary), UseCaseError> {
    let invitation = match lock {
        Lock::None => persistence::find_invitation_for_addressee(connection, id).await?,
        Lock::Row => persistence::lock_invitation(connection, id).await?,
    }
    .ok_or(UseCaseError::NotFound)?;
    let user = ledgerly_identity::find_user(connection, caller)
        .await?
        .ok_or(UseCaseError::NotFound)?;
    if normalise_email(&user.email) == invitation.email {
        Ok((invitation, user))
    } else {
        Err(UseCaseError::NotFound)
    }
}

/// Whether [`addressed`] locks the invitation's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lock {
    /// Accepting locks the team first, and the invitation only then.
    None,
    Row,
}

/// Expired is [`UseCaseError::InvitationExpired`]; answered or revoked is
/// [`UseCaseError::InvitationNotPending`]. Java: `InvitationResponses.requirePending`.
fn require_pending(invitation: &Invitation, now: DateTime<Utc>) -> Result<(), UseCaseError> {
    match invitation.status(now) {
        InvitationStatus::Pending => Ok(()),
        InvitationStatus::Expired => Err(UseCaseError::InvitationExpired),
        status => Err(UseCaseError::InvitationNotPending(format!(
            "This invitation is {status}"
        ))),
    }
}
