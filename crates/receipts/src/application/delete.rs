//! Java counterpart: `receipts/internal/application/DeleteReceipt.java` and `ExpenseDeletedCleanup.java`.

use ledgerly_expenses::ExpenseDeletedListener;
use ledgerly_shared::id::{ExpenseId, ReceiptId, TeamId, UserId};

use super::{Context, UseCaseError};
use crate::events::{ReceiptEvent, ReceiptEventKind};
use crate::persistence;

/// Lets the claimant remove a receipt from their draft expense; the file goes with it.
#[derive(Debug, Clone)]
pub(crate) struct DeleteReceipt {
    context: Context,
}

impl DeleteReceipt {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// The row and the `Deleted` event in one transaction, then the object. If removing the object fails, the
    /// row is already gone, so the file is unreachable; a leftover object only costs space.
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        expense: ExpenseId,
        id: ReceiptId,
        caller: UserId,
    ) -> Result<(), UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        self.context
            .expenses
            .require_can_change_receipts(&mut transaction, team, expense, caller)
            .await?;
        let receipt = persistence::find_on_expense(&mut transaction, team, expense, id)
            .await?
            .ok_or(UseCaseError::NotFound)?;
        if !persistence::delete(&mut transaction, team, id).await? {
            return Err(UseCaseError::NotFound);
        }
        let deleted = ReceiptEvent::new(
            team,
            expense,
            id,
            self.context.clock.now(),
            ReceiptEventKind::Deleted { actor_id: caller },
        );
        self.context.events.publish(&mut transaction, &deleted).await?;
        transaction.commit().await?;
        if let Err(error) = self.context.storage.delete(&receipt.storage_key).await {
            tracing::warn!("Could not delete object of receipt {id}: {error}");
        }
        Ok(())
    }
}

/// When an expense is deleted, removes its receipts and their files. Java's `@ApplicationModuleListener`
/// runs after the deletion commits, in a transaction of its own; this one runs in a task the deletion starts.
#[derive(Debug, Clone)]
pub(crate) struct ExpenseDeletedCleanup {
    context: Context,
}

impl ExpenseDeletedCleanup {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// The rows first, then the files; a file that cannot be removed is logged and not retried.
    pub(crate) async fn execute(&self, team: TeamId, expense: ExpenseId) -> Result<(), sqlx::Error> {
        let mut transaction = self.context.pool.begin().await?;
        let keys = persistence::delete_all_of_expense(&mut transaction, team, expense).await?;
        transaction.commit().await?;
        for key in keys {
            if let Err(error) = self.context.storage.delete(&key).await {
                tracing::warn!("Could not delete an object of deleted expense {expense}: {error}");
            }
        }
        Ok(())
    }
}

impl ExpenseDeletedListener for ExpenseDeletedCleanup {
    fn expense_deleted(&self, team: TeamId, expense: ExpenseId) {
        let cleanup = self.clone();
        tokio::spawn(async move {
            if let Err(error) = cleanup.execute(team, expense).await {
                tracing::warn!("The receipts of deleted expense {expense} could not be removed: {error}");
            }
        });
    }
}
