//! The two checks every use case starts with: is the caller a teammate, and may they see this expense.
//!
//! Java counterpart: `expenses/internal/application/ExpenseAccess.java`.

use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::version::{Version, Versioned};
use sqlx::PgConnection;

use super::{MembershipLookup, UseCaseError};
use crate::domain::{Actor, Expense};
use crate::persistence;

/// The caller as an actor with their role. Not a teammate: [`UseCaseError::NotFound`], so that the team's
/// existence is not revealed.
pub(crate) async fn actor(
    connection: &mut PgConnection,
    memberships: &dyn MembershipLookup,
    team: TeamId,
    user: UserId,
) -> Result<Actor, UseCaseError> {
    let role = memberships
        .role_of(connection, team, user)
        .await?
        .ok_or(UseCaseError::NotFound)?;
    Ok(Actor::new(user, role))
}

/// The expense, if it is in this team and the actor may see it. Missing, another team's, or a colleague's for
/// a member: all [`UseCaseError::NotFound`].
pub(crate) async fn load(
    connection: &mut PgConnection,
    actor: Actor,
    team: TeamId,
    id: ExpenseId,
) -> Result<Versioned<Expense>, UseCaseError> {
    persistence::find(connection, team, id)
        .await?
        .filter(|found| found.value.is_visible_to(actor))
        .ok_or(UseCaseError::NotFound)
}

/// The version the client read must be the current one. Java checks this before the domain: a stale `If-Match`
/// is 412 even for someone who may not edit.
pub(crate) fn require_version(found: &Versioned<Expense>, expected: Version) -> Result<(), UseCaseError> {
    if found.version == expected {
        Ok(())
    } else {
        Err(UseCaseError::Stale)
    }
}
