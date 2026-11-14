//! The role check in front of the team endpoints. Java: `TeamAuthorization`, called from
//! `@PreAuthorize("@teamAuthorization.anyOf(#teamId, 'OWNER')")` after Spring has resolved the arguments.

use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::Role;
use sqlx::PgPool;

use super::UseCaseError;
use crate::persistence;

/// Reads the caller's role from the database on every call, so that a demotion takes effect on the next
/// request: not a teammate is [`UseCaseError::NotFound`] (the team's existence is not revealed), a teammate
/// with the wrong role [`UseCaseError::Forbidden`].
#[derive(Debug, Clone)]
pub(crate) struct TeamAuthorization {
    pool: PgPool,
}

impl TeamAuthorization {
    pub(crate) const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The caller holds exactly one of `allowed`. Java: `anyOf`.
    pub(crate) async fn any_of(&self, team: TeamId, caller: UserId, allowed: &[Role]) -> Result<(), UseCaseError> {
        let actual = self.role_of_caller(team, caller).await?;
        if allowed.contains(&actual) {
            Ok(())
        } else {
            Err(refused(actual))
        }
    }

    /// The caller holds `role` or a higher one on the ladder. Java: `atLeast`.
    pub(crate) async fn at_least(&self, team: TeamId, caller: UserId, role: Role) -> Result<(), UseCaseError> {
        let actual = self.role_of_caller(team, caller).await?;
        if actual.includes(role) {
            Ok(())
        } else {
            Err(refused(actual))
        }
    }

    async fn role_of_caller(&self, team: TeamId, caller: UserId) -> Result<Role, UseCaseError> {
        let mut connection = self.pool.acquire().await?;
        persistence::role_of(&mut connection, team, caller)
            .await?
            .ok_or(UseCaseError::NotFound)
    }
}

/// Java: `new Forbidden("Role " + actual + " may not do this")`.
fn refused(actual: Role) -> UseCaseError {
    UseCaseError::Forbidden(format!("Role {actual} may not do this"))
}
