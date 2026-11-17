//! The use cases of the imports module.
//!
//! Java counterpart: `imports/internal/application/` and the job in `internal/infrastructure/`.

mod get;
mod runner;
mod upload;

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use ledgerly_expenses::{ExpenseRefusal, ExpensesFacade, NewExpense, ValidationProblem};
use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::Limits;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::storage::{ObjectStorage, StorageError};
use ledgerly_shared::web::Problem;
use ledgerly_teams::TeamsFacade;
use sqlx::{PgConnection, PgPool};

pub(crate) use get::{GetImport, ImportView};
pub(crate) use runner::{ImportQueuedListener, ImportRunner};
pub(crate) use upload::UploadImport;

use crate::domain::Refusal;

/// What the job needs of the expenses module: the expense rules, and the one way to create drafts.
pub(crate) trait Drafts: fmt::Debug + Send + Sync {
    fn validate(&self, expense: &NewExpense) -> Option<ValidationProblem>;

    /// Creates the drafts in the caller's transaction.
    fn create<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        team: TeamId,
        claimant: UserId,
        drafts: &'a [NewExpense],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ExpenseId>, ExpenseRefusal>> + Send + 'a>>;
}

impl Drafts for ExpensesFacade {
    fn validate(&self, expense: &NewExpense) -> Option<ValidationProblem> {
        Self::validate(self, expense)
    }

    fn create<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        team: TeamId,
        claimant: UserId,
        drafts: &'a [NewExpense],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ExpenseId>, ExpenseRefusal>> + Send + 'a>> {
        Box::pin(self.create_drafts(connection, team, claimant, drafts))
    }
}

/// What every use case works with.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    pub pool: PgPool,
    pub clock: Arc<dyn Clock>,
    pub events: Arc<dyn Publisher>,
    pub storage: Arc<dyn ObjectStorage>,
    pub teams: TeamsFacade,
    pub drafts: Arc<dyn Drafts>,
    pub limits: Limits,
}

/// Why a use case did not complete. The web layer turns each into Java's answer.
#[derive(Debug, thiserror::Error)]
pub(crate) enum UseCaseError {
    /// Not a teammate, or not an import the caller may see. Java: `NotFound`.
    #[error("not found")]
    NotFound,
    /// The daily allowance is used up. Java: `TooManyRequests`, with `Retry-After` until UTC midnight.
    #[error("At most {limit} imports per team per day")]
    DailyLimit { limit: u32, retry_after_seconds: i64 },
    /// The upload could not be read as Spring would (no file part, over Spring's limits, not multipart).
    #[error("the upload was refused")]
    Upload(Problem),
    /// Larger than `ledgerly.limits.max-upload-size`, in whole MiB. Java: `CappedInputStream`.
    #[error("the file is larger than {0} MB")]
    TooLarge(u64),
    /// The file is not an import Java would accept.
    #[error("the file was refused")]
    Refused(Refusal),
    #[error("the upload could not be staged: {0}")]
    Staging(#[source] std::io::Error),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}
