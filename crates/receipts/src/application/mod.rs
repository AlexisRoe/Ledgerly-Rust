//! The use cases of the receipts module.
//!
//! Java counterpart: `receipts/internal/application/`. Every use case asks the expenses module first whether
//! the caller may see the expense, or change its receipts (`ExpensesFacade`), so the receipts module never
//! decides that itself.

mod list;
mod scan;
mod upload;

use std::sync::Arc;

use ledgerly_expenses::{ExpenseRefusal, ExpensesFacade};
use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::storage::{ObjectStorage, StorageError};
use ledgerly_shared::web::Problem;
pub(crate) use list::ListReceipts;
pub(crate) use scan::{RETRY_DELAYS, ScanReceipt};
use sqlx::PgPool;
pub(crate) use upload::UploadReceipt;

/// What every use case works with.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    pub pool: PgPool,
    pub clock: Arc<dyn Clock>,
    pub events: Arc<dyn Publisher>,
    pub storage: Arc<dyn ObjectStorage>,
    pub expenses: ExpensesFacade,
}

/// Why a use case did not complete. The web layer turns each into Java's answer.
#[derive(Debug, thiserror::Error)]
pub(crate) enum UseCaseError {
    /// The expenses module refused: 404, 403 or 409 `EXPENSE_NOT_DRAFT`.
    #[error(transparent)]
    Expense(#[from] ExpenseRefusal),
    /// The upload itself could not be read as Spring would: no file part (400), a part or the request over
    /// Spring's limits (413), or not multipart at all (500).
    #[error("the upload was refused")]
    Upload(Problem),
    /// No receipt of that id on that expense, or not one that may be served. Java: `NotFound`.
    #[error("not found")]
    #[expect(dead_code, reason = "the download and delete arrive in M7.4")]
    NotFound,
    /// The file has no content. Java: `InvalidRequest("file", "The file is empty")`.
    #[error("the file is empty")]
    Empty,
    /// The content is larger than `ledgerly.limits.max-upload-size`, in whole MiB. Java: `CappedInputStream`.
    #[error("the file is larger than {0} MB")]
    TooLarge(u64),
    /// The bytes are not a PDF, JPEG or PNG. Java: `UnsupportedMedia`.
    #[error("only PDF, JPEG and PNG files are accepted")]
    UnsupportedType,
    /// The same content is already on the expense. Java: `Conflict(RECEIPT_DUPLICATE)`.
    #[error("the file is already attached to the expense")]
    Duplicate,
    /// The upload could not be staged on local disk.
    #[error("the upload could not be staged: {0}")]
    Staging(#[source] std::io::Error),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}
