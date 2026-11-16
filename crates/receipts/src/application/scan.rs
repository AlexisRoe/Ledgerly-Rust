//! Java counterpart: `receipts/internal/application/ScanReceipt.java`, with the retries of
//! `ClamAvScanner`'s `@Retryable(maxRetries = 3, delay = 500, multiplier = 2)`.

use std::sync::Arc;
use std::time::Duration;

use ledgerly_shared::id::{ReceiptId, TeamId};

use super::Context;
use crate::domain::{Receipt, ScanStatus};
use crate::events::{ReceiptEvent, ReceiptEventKind};
use crate::persistence;
use crate::scanner::{MalwareScanner, ScanError, Verdict};

/// The waits before the second, third and fourth attempt: 500 ms, doubled each time.
pub(crate) const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_millis(1000),
    Duration::from_millis(2000),
];

/// Scans a stored receipt: `PENDING_SCAN` becomes `ACCEPTED` or `QUARANTINED`, or stays when the scanner
/// cannot answer.
///
/// The outbox delivers the upload to it once the upload has committed ([`super::ScanListener`]). A scan that
/// fails, or a process that stops meanwhile, leaves the publication open, and the upload is delivered again:
/// at the next start, or after two minutes.
#[derive(Debug, Clone)]
pub(crate) struct ScanReceipt {
    context: Context,
    scanner: Arc<dyn MalwareScanner>,
    delays: [Duration; 3],
}

/// Why a scan did not finish. The receipt stays `PENDING_SCAN`.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ScanFailure {
    #[error("{}", .0.kind())]
    Scanner(#[from] ScanError),
    #[error("object storage")]
    Storage(#[from] ledgerly_shared::storage::StorageError),
    #[error("database")]
    Database(#[from] sqlx::Error),
}

impl ScanReceipt {
    pub(crate) fn new(context: Context, scanner: Arc<dyn MalwareScanner>, delays: [Duration; 3]) -> Self {
        Self {
            context,
            scanner,
            delays,
        }
    }

    /// Scans the receipt if it is still pending, and does nothing otherwise, so running it twice for one
    /// receipt is harmless.
    pub(crate) async fn execute(&self, team: TeamId, id: ReceiptId) -> Result<(), ScanFailure> {
        let found = {
            let mut connection = self.context.pool.acquire().await?;
            persistence::find(&mut connection, team, id).await?
        };
        let Some(receipt) = found.filter(|receipt| receipt.scan_status == ScanStatus::PendingScan) else {
            return Ok(());
        };
        let verdict = match self.verdict(&receipt).await {
            Ok(verdict) => verdict,
            Err(failure) => {
                tracing::warn!("Scan of receipt {id} failed ({failure}); it stays PENDING_SCAN");
                return Err(failure);
            }
        };
        match verdict {
            Verdict::Clean => self.accept(&receipt).await,
            Verdict::Infected { signature } => self.quarantine(&receipt, &signature).await,
        }
    }

    /// Up to four attempts; the object is opened afresh for each, so a retry scans the whole file.
    async fn verdict(&self, receipt: &Receipt) -> Result<Verdict, ScanFailure> {
        let mut delays = self.delays.iter();
        loop {
            let mut content = self.context.storage.open(&receipt.storage_key).await?;
            match self.scanner.scan(&mut content).await {
                Ok(verdict) => return Ok(verdict),
                Err(error) => match delays.next() {
                    Some(delay) => tokio::time::sleep(*delay).await,
                    None => return Err(error.into()),
                },
            }
        }
    }

    async fn accept(&self, receipt: &Receipt) -> Result<(), ScanFailure> {
        let mut transaction = self.context.pool.begin().await?;
        if persistence::mark_accepted(&mut transaction, receipt.team_id, receipt.id).await? {
            let accepted = self.event(receipt, ReceiptEventKind::Accepted);
            self.context.events.publish(&mut transaction, &accepted).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// The row changes first. If moving the object then fails, the receipt is already `QUARANTINED` (never
    /// served) and a second scan finds nothing to do; at worst a stray object stays under its old key,
    /// unreachable from the API.
    async fn quarantine(&self, receipt: &Receipt, signature: &str) -> Result<(), ScanFailure> {
        let quarantine_key = format!("quarantine/{}", uuid::Uuid::new_v4());
        let mut transaction = self.context.pool.begin().await?;
        let changed =
            persistence::mark_quarantined(&mut transaction, receipt.team_id, receipt.id, &quarantine_key).await?;
        if changed {
            let quarantined = self.event(receipt, ReceiptEventKind::Quarantined);
            self.context.events.publish(&mut transaction, &quarantined).await?;
        }
        transaction.commit().await?;
        if !changed {
            return Ok(());
        }
        // Java also counts it (`ledgerly.receipts.quarantined`); the metrics arrive in M10.3.
        tracing::warn!("Receipt {} quarantined: scanner reported {signature}", receipt.id);
        if let Err(error) = self
            .context
            .storage
            .relocate(&receipt.storage_key, &quarantine_key)
            .await
        {
            tracing::warn!(
                "Could not move the object of quarantined receipt {}: {error}",
                receipt.id
            );
            return Err(error.into());
        }
        Ok(())
    }

    fn event(&self, receipt: &Receipt, kind: ReceiptEventKind) -> ReceiptEvent {
        ReceiptEvent::new(
            receipt.team_id,
            receipt.expense_id,
            receipt.id,
            self.context.clock.now(),
            kind,
        )
    }
}
