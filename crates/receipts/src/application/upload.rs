//! Java counterpart: `receipts/internal/application/UploadReceipt.java`.

use ledgerly_shared::id::{ExpenseId, ReceiptId, TeamId, UserId};
use ledgerly_shared::web::{MultipartUpload, UploadedFile};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;

use super::{Context, ScanReceipt, UseCaseError};
use crate::domain::{DETECTION_WINDOW, DetectedType, Receipt, ScanStatus, display_name};
use crate::events::{ReceiptEvent, ReceiptEventKind};
use crate::persistence::{self, InsertError};

const MEBIBYTE: u64 = 1024 * 1024;

/// The steps of an upload: may the caller upload here; stream the file to a temporary file while hashing it
/// and counting its bytes; detect its type from its bytes; refuse the same file twice; store it under a random
/// key; record it as `PENDING_SCAN`.
#[derive(Debug, Clone)]
pub(crate) struct UploadReceipt {
    context: Context,
    scan: ScanReceipt,
    max_upload_size: u64,
}

/// The file on local disk, removed when this is dropped, whatever happened.
struct Staged {
    file: NamedTempFile,
    size: u64,
    sha256: String,
    detected: Option<DetectedType>,
}

impl UploadReceipt {
    /// `max_upload_size`: `ledgerly.limits.max-upload-size`, in bytes.
    pub(crate) const fn new(context: Context, scan: ScanReceipt, max_upload_size: u64) -> Self {
        Self {
            context,
            scan,
            max_upload_size,
        }
    }

    /// Stores the file and returns the receipt in `PENDING_SCAN`; its scan starts once it is recorded.
    ///
    /// Nothing of the body is read before the expenses module has said that the caller may change this
    /// expense's receipts (plan M7.2; Spring reads the whole request before Java's controller runs).
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        expense: ExpenseId,
        caller: UserId,
        upload: MultipartUpload,
    ) -> Result<Receipt, UseCaseError> {
        {
            let mut connection = self.context.pool.acquire().await?;
            self.context
                .expenses
                .require_can_change_receipts(&mut connection, team, expense, caller)
                .await?;
        }

        let file = upload.file("file").await.map_err(UseCaseError::Upload)?;
        let original_name = display_name(file.original_name());
        let staged = self.stage(file).await?;
        if staged.size == 0 {
            return Err(UseCaseError::Empty);
        }
        let Some(detected) = staged.detected else {
            return Err(UseCaseError::UnsupportedType);
        };

        let possible_duplicate = {
            let mut connection = self.context.pool.acquire().await?;
            if persistence::is_on_expense(&mut connection, team, expense, &staged.sha256).await? {
                return Err(UseCaseError::Duplicate);
            }
            persistence::is_on_another_expense(&mut connection, team, expense, &staged.sha256).await?
        };

        // A random key: the client's file name never reaches the object store as a path.
        let key = format!("receipts/{}", uuid::Uuid::new_v4());
        self.context
            .storage
            .put(&key, staged.file.path(), detected.mime())
            .await?;
        let receipt = Receipt {
            id: ReceiptId::new_v7(),
            team_id: team,
            expense_id: expense,
            storage_key: key,
            original_name,
            detected_type: detected,
            size_bytes: i64::try_from(staged.size).unwrap_or(i64::MAX),
            sha256: staged.sha256,
            scan_status: ScanStatus::PendingScan,
            possible_duplicate,
            uploaded_by: caller,
            created_at: self.context.clock.now(),
        };
        match self.record(&receipt).await {
            Ok(()) => {
                self.scan.start(receipt.team_id, receipt.id);
                Ok(receipt)
            }
            Err(error) => {
                self.remove_object(&receipt.storage_key).await;
                Err(error)
            }
        }
    }

    /// Inserts the row and publishes `Uploaded`, in one transaction.
    async fn record(&self, receipt: &Receipt) -> Result<(), UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        persistence::insert(&mut transaction, receipt)
            .await
            .map_err(|error| match error {
                // Lost the race with an identical upload: the unique index decided.
                InsertError::Duplicate => UseCaseError::Duplicate,
                InsertError::Database(error) => UseCaseError::Database(error),
            })?;
        let uploaded = ReceiptEvent::new(
            receipt.team_id,
            receipt.expense_id,
            receipt.id,
            self.context.clock.now(),
            ReceiptEventKind::Uploaded {
                actor_id: receipt.uploaded_by,
            },
        );
        self.context.events.publish(&mut transaction, &uploaded).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// The object of an upload that was not recorded. A leftover object is unreachable, so failing to remove
    /// it only costs space.
    async fn remove_object(&self, key: &str) {
        if let Err(error) = self.context.storage.delete(key).await {
            tracing::warn!("Could not remove the object of a failed upload: {error}");
        }
    }

    /// Streams the file to a temporary file, hashing it and keeping its first bytes for the type.
    ///
    /// Over `max_upload_size`, the rest of the part is still read and dropped: Spring would have refused a
    /// part over its own 10 MiB limit before Java's cap was reached, and that answer has to win here too.
    async fn stage(&self, mut file: UploadedFile) -> Result<Staged, UseCaseError> {
        let staged = tempfile::Builder::new()
            .prefix("receipt-")
            .suffix(".upload")
            .tempfile()
            .map_err(UseCaseError::Staging)?;
        let mut writer = tokio::fs::File::from_std(staged.reopen().map_err(UseCaseError::Staging)?);
        let mut hasher = Sha256::new();
        let mut head = Vec::with_capacity(DETECTION_WINDOW);
        let mut size: u64 = 0;
        let mut over_the_limit = false;
        while let Some(chunk) = file.chunk().await.map_err(UseCaseError::Upload)? {
            if over_the_limit {
                continue;
            }
            size += chunk.len() as u64;
            if size > self.max_upload_size {
                over_the_limit = true;
                continue;
            }
            hasher.update(&chunk);
            let missing = DETECTION_WINDOW.saturating_sub(head.len());
            head.extend_from_slice(chunk.get(..missing.min(chunk.len())).unwrap_or_default());
            writer.write_all(&chunk).await.map_err(UseCaseError::Staging)?;
        }
        if over_the_limit {
            return Err(UseCaseError::TooLarge(self.max_upload_size / MEBIBYTE));
        }
        writer.flush().await.map_err(UseCaseError::Staging)?;
        Ok(Staged {
            file: staged,
            size,
            sha256: hex(&hasher.finalize()),
            detected: DetectedType::detect(&head),
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}
