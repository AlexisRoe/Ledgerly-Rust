//! Java counterpart: `receipts/internal/application/DownloadReceipt.java`.

use std::time::Duration;

use chrono::{DateTime, Utc};
use ledgerly_shared::id::{ExpenseId, ReceiptId, TeamId, UserId};

use super::{Context, UseCaseError};
use crate::domain::download_name;
use crate::persistence;

/// Five minutes: long enough to click, short enough that a leaked link soon dies. Java: `LINK_TTL`.
pub(crate) const LINK_TTL: Duration = Duration::from_secs(5 * 60);

/// A link to fetch the file from the object store, valid until `expires_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Link {
    pub url: String,
    pub expires_at: DateTime<Utc>,
}

/// A short-lived, signed link to an accepted receipt. The object store serves the file, not the app.
#[derive(Debug, Clone)]
pub(crate) struct DownloadReceipt {
    context: Context,
}

impl DownloadReceipt {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// Only an `ACCEPTED` receipt. A pending or quarantined one is a 404, as if it did not exist. The link
    /// downloads the file as an attachment under its sanitised name, with the detected type.
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        expense: ExpenseId,
        id: ReceiptId,
        caller: UserId,
    ) -> Result<Link, UseCaseError> {
        let receipt = {
            let mut transaction = self.context.pool.begin().await?;
            self.context
                .expenses
                .require_can_view(&mut transaction, team, expense, caller)
                .await?;
            let found = persistence::find_on_expense(&mut transaction, team, expense, id).await?;
            transaction.commit().await?;
            found
                .filter(crate::domain::Receipt::is_downloadable)
                .ok_or(UseCaseError::NotFound)?
        };
        let name = download_name(&receipt.original_name, Some(receipt.detected_type));
        let url = self
            .context
            .storage
            .presign_get(&receipt.storage_key, &name, receipt.detected_type.mime(), LINK_TTL)
            .await?;
        let expires_at =
            self.context.clock.now() + chrono::Duration::from_std(LINK_TTL).unwrap_or(chrono::Duration::zero());
        Ok(Link { url, expires_at })
    }
}
