//! Java counterpart: `expenses/internal/application/DeleteExpense.java`.

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::version::Version;
use sqlx::PgPool;

use super::{MembershipLookup, UseCaseError, access};
use crate::events::{ExpenseEvent, ExpenseEventKind};
use crate::persistence;

/// Deletes a draft; only its claimant may, and only with the current version. `ExpenseDeleted` is published in
/// the same transaction; receipts removes the expense's receipts when it is delivered.
#[derive(Debug, Clone)]
pub(crate) struct DeleteExpense {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    memberships: Arc<dyn MembershipLookup>,
    events: Arc<dyn Publisher>,
}

impl DeleteExpense {
    pub(crate) fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        memberships: Arc<dyn MembershipLookup>,
        events: Arc<dyn Publisher>,
    ) -> Self {
        Self {
            pool,
            clock,
            memberships,
            events,
        }
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
        let deleted = ExpenseEvent::new(team, id, caller, self.clock.now(), ExpenseEventKind::Deleted);
        self.events.publish(&mut transaction, &deleted).await?;
        transaction.commit().await?;
        Ok(())
    }
}
