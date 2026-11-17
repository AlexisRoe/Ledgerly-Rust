//! Changing a role, removing a member and leaving. Java: `ChangeMemberRole`, `RemoveMember`, `LeaveTeam` in
//! `teams/internal/application/`.
//!
//! All three hold the team row lock while they count owners, so two owners demoting, removing or leaving each
//! other at the same moment run one after the other, and the second finds the first's change: a team always
//! keeps at least one OWNER.

use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::Role;

use super::{Context, UseCaseError, guard};
use crate::events::{TeamEvent, TeamEventKind};
use crate::persistence;

const KEEP_AN_OWNER: &str = "A team must keep at least one owner";

/// Lets an OWNER change another member's role; the last OWNER cannot be demoted.
#[derive(Debug, Clone)]
pub(crate) struct ChangeMemberRole {
    context: Context,
}

impl ChangeMemberRole {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// The member's new role. Setting the role they already have changes nothing but is still published, as
    /// in Java.
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        caller: UserId,
        target: UserId,
        new_role: Role,
    ) -> Result<Role, UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        guard::lock_and_require_owner(&mut transaction, team, caller).await?;
        let old_role = guard::require_member(&mut transaction, team, target).await?;
        if old_role == Role::Owner && new_role != Role::Owner && guard::is_last_owner(&mut transaction, team).await? {
            return Err(UseCaseError::LastOwner(KEEP_AN_OWNER));
        }
        persistence::update_role(&mut transaction, team, target, new_role).await?;
        let changed = TeamEventKind::MemberRoleChanged {
            member_id: target,
            old_role,
            new_role,
        };
        let now = self.context.clock.now();
        self.context
            .events
            .publish(&mut transaction, &TeamEvent::new(team, caller, now, changed))
            .await?;
        transaction.commit().await?;
        Ok(new_role)
    }
}

/// Lets an OWNER remove another member; the last OWNER cannot be removed.
#[derive(Debug, Clone)]
pub(crate) struct RemoveMember {
    context: Context,
}

impl RemoveMember {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    pub(crate) async fn execute(&self, team: TeamId, caller: UserId, target: UserId) -> Result<(), UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        guard::lock_and_require_owner(&mut transaction, team, caller).await?;
        let role = guard::require_member(&mut transaction, team, target).await?;
        if role == Role::Owner && guard::is_last_owner(&mut transaction, team).await? {
            return Err(UseCaseError::LastOwner(KEEP_AN_OWNER));
        }
        persistence::delete_membership(&mut transaction, team, target).await?;
        let removed = TeamEventKind::MemberRemoved { member_id: target };
        let now = self.context.clock.now();
        self.context
            .events
            .publish(&mut transaction, &TeamEvent::new(team, caller, now, removed))
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

/// Lets a member leave; the last OWNER may not.
#[derive(Debug, Clone)]
pub(crate) struct LeaveTeam {
    context: Context,
}

impl LeaveTeam {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// Not a teammate: 404 before any lock is taken.
    pub(crate) async fn execute(&self, team: TeamId, caller: UserId) -> Result<(), UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        guard::require_member(&mut transaction, team, caller).await?;
        guard::lock(&mut transaction, team).await?;
        let role = guard::require_member(&mut transaction, team, caller).await?;
        if role == Role::Owner && guard::is_last_owner(&mut transaction, team).await? {
            return Err(UseCaseError::LastOwner(
                "The last owner cannot leave; promote someone else first",
            ));
        }
        persistence::delete_membership(&mut transaction, team, caller).await?;
        let now = self.context.clock.now();
        self.context
            .events
            .publish(
                &mut transaction,
                &TeamEvent::new(team, caller, now, TeamEventKind::MemberLeft),
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}
