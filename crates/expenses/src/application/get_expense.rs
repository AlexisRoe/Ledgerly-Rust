//! Java counterpart: `expenses/internal/application/GetExpense.java`.

use ledgerly_shared::id::{ExpenseId, TeamId};
use ledgerly_shared::version::Versioned;
use sqlx::PgPool;

use super::UseCaseError;
use crate::domain::Expense;
use crate::persistence;

/// Reads one expense.
#[derive(Debug, Clone)]
pub(crate) struct GetExpense {
    pool: PgPool,
}

impl GetExpense {
    pub(crate) const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The expense with this id in this team, or `None`, whether it does not exist or belongs to another team.
    ///
    /// This is Java's raw `execute(teamId, id)`, scoped to the team only. The caller-aware lookup, which also
    /// answers "not found" to someone who may not see the expense, comes with the membership check (M3.3).
    ///
    /// Runs in a read-only transaction, as Java's `@Transactional(readOnly = true)` does.
    ///
    /// # Errors
    ///
    /// [`UseCaseError::Storage`] if the database fails or the stored row is corrupt.
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        id: ExpenseId,
    ) -> Result<Option<Versioned<Expense>>, UseCaseError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("set transaction read only")
            .execute(&mut *transaction)
            .await?;
        let found = persistence::find(&mut transaction, team, id).await?;
        transaction.commit().await?;
        Ok(found)
    }
}
