//! Java counterpart: `expenses/internal/application/DeleteExpense.java`.

use std::sync::Arc;

use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::version::Version;
use sqlx::PgPool;

use super::{MembershipLookup, UseCaseError, access};
use crate::persistence;

/// Deletes a draft; only its claimant may, and only with the current version.
#[derive(Debug, Clone)]
pub(crate) struct DeleteExpense {
    pool: PgPool,
    memberships: Arc<dyn MembershipLookup>,
}

impl DeleteExpense {
    pub(crate) fn new(pool: PgPool, memberships: Arc<dyn MembershipLookup>) -> Self {
        Self { pool, memberships }
    }

    /// # Errors
    ///
    /// Not visible (404), stale version (412), not the claimant (403), not a draft (409), or a storage failure.
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        id: ExpenseId,
        caller: UserId,
        expected: Version,
    ) -> Result<(), UseCaseError> {
        let mut transaction = self.pool.begin().await?;
        let actor = access::actor(&mut transaction, self.memberships.as_ref(), team, caller).await?;
        let found = access::load(&mut transaction, actor, team, id).await?;
        access::require_version(&found, expected)?;
        found.value.require_deletable_by(actor)?;
        persistence::delete(&mut transaction, team, id, found.version).await?;
        transaction.commit().await?;
        Ok(())
    }
}
