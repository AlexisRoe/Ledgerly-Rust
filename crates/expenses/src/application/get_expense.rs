//! Java counterpart: `expenses/internal/application/GetExpense.java`.

use std::sync::Arc;

use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::version::Versioned;
use sqlx::PgPool;

use super::{MembershipLookup, UseCaseError, access};
use crate::domain::Expense;

/// Reads one expense. A teammate who may not see it gets the same 404 as one that does not exist.
#[derive(Debug, Clone)]
pub(crate) struct GetExpense {
    pool: PgPool,
    memberships: Arc<dyn MembershipLookup>,
}

impl GetExpense {
    pub(crate) fn new(pool: PgPool, memberships: Arc<dyn MembershipLookup>) -> Self {
        Self { pool, memberships }
    }

    /// Runs in a read-only transaction, as Java's `@Transactional(readOnly = true)` does.
    ///
    /// # Errors
    ///
    /// [`UseCaseError::NotFound`] unless the caller is a teammate who may see the expense, or a storage failure.
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        id: ExpenseId,
        caller: UserId,
    ) -> Result<Versioned<Expense>, UseCaseError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("set transaction read only")
            .execute(&mut *transaction)
            .await?;
        let actor = access::actor(&mut transaction, self.memberships.as_ref(), team, caller).await?;
        let found = access::load(&mut transaction, actor, team, id).await?;
        transaction.commit().await?;
        Ok(found)
    }
}
