//! Java counterpart: `receipts/internal/application/ListReceipts.java`.

use ledgerly_shared::id::{ExpenseId, TeamId, UserId};

use super::{Context, UseCaseError};
use crate::domain::Receipt;
use crate::persistence;

/// An expense's receipts, whatever their scan status, for whoever may see the expense.
#[derive(Debug, Clone)]
pub(crate) struct ListReceipts {
    context: Context,
}

impl ListReceipts {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// Oldest first. This is the endpoint a client polls after an upload.
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        expense: ExpenseId,
        caller: UserId,
    ) -> Result<Vec<Receipt>, UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        self.context
            .expenses
            .require_can_view(&mut transaction, team, expense, caller)
            .await?;
        let receipts = persistence::list(&mut transaction, team, expense).await?;
        transaction.commit().await?;
        Ok(receipts)
    }
}
