//! Java counterpart: `receipts/internal/application/ReceiptChecksImpl.java`: receipts' side of the submit rule.

use ledgerly_expenses::{ReceiptChecks, ReceiptSummary, SummaryFuture};
use ledgerly_shared::id::{ExpenseId, TeamId};
use sqlx::PgConnection;

use crate::persistence;

/// Counts the expense's receipts by scan status, on the submit's own connection.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ReceiptCounts;

impl ReceiptChecks for ReceiptCounts {
    fn summary_for<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        team_id: TeamId,
        expense_id: ExpenseId,
    ) -> SummaryFuture<'a> {
        Box::pin(async move {
            let (accepted, pending) = persistence::count_by_status(connection, team_id, expense_id).await?;
            let count = |n: i64| u32::try_from(n).unwrap_or(u32::MAX);
            Ok(ReceiptSummary::new(count(accepted), count(pending)))
        })
    }
}
