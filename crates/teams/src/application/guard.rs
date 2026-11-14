//! The checks shared by the use cases that change who belongs to a team. Java: `TeamGuard`.

use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::Role;
use sqlx::PgConnection;

use super::UseCaseError;
use crate::persistence;

/// The user's role in the team. Not a teammate: [`UseCaseError::NotFound`], so that the team's existence is
/// not revealed.
pub(super) async fn require_member(
    connection: &mut PgConnection,
    team: TeamId,
    user: UserId,
) -> Result<Role, UseCaseError> {
    persistence::role_of(connection, team, user)
        .await?
        .ok_or(UseCaseError::NotFound)
}

/// Locks the team row, then checks that the caller is (still) an OWNER. The check must come after the lock: a
/// caller demoted by a concurrent request has to see that demotion.
pub(super) async fn lock_and_require_owner(
    connection: &mut PgConnection,
    team: TeamId,
    caller: UserId,
) -> Result<(), UseCaseError> {
    lock(connection, team).await?;
    if require_member(connection, team, caller).await? == Role::Owner {
        Ok(())
    } else {
        Err(UseCaseError::Forbidden("Only an owner may do this".to_owned()))
    }
}

/// Locks the team row, for a caller who may hold any role. A team that does not exist is
/// [`UseCaseError::NotFound`].
pub(super) async fn lock(connection: &mut PgConnection, team: TeamId) -> Result<(), UseCaseError> {
    if persistence::lock_team(connection, team).await? {
        Ok(())
    } else {
        Err(UseCaseError::NotFound)
    }
}

/// True when at most one OWNER is left, so that removing or demoting an owner would orphan the team. Only
/// meaningful under the team lock.
pub(super) async fn is_last_owner(connection: &mut PgConnection, team: TeamId) -> Result<bool, UseCaseError> {
    Ok(persistence::count_owners(connection, team).await? <= 1)
}
