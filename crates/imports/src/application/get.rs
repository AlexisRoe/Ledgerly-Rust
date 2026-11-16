//! Java counterpart: `imports/internal/application/GetImport.java`.

use ledgerly_shared::id::{ImportId, TeamId, UserId};
use ledgerly_shared::role::Role;

use super::{Context, UseCaseError};
use crate::domain::ImportStatus;
use crate::persistence::{self, RowError};

/// At most this many row errors are shown. Java: `GetImport.MAX_ERRORS_SHOWN`.
const MAX_ERRORS_SHOWN: i64 = 1000;

/// An import's progress and its first row errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportView {
    pub id: ImportId,
    pub status: ImportStatus,
    pub total_rows: i32,
    pub imported_rows: i32,
    pub skipped_rows: i64,
    /// By line number, at most 1000.
    pub errors: Vec<RowError>,
    pub errors_truncated: bool,
}

/// Visible to the user who started the import and to the team's OWNERs; everyone else, in the team or not,
/// gets the same 404 as for an id that does not exist.
#[derive(Debug, Clone)]
pub(crate) struct GetImport {
    context: Context,
}

impl GetImport {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    pub(crate) async fn execute(&self, team: TeamId, id: ImportId, caller: UserId) -> Result<ImportView, UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        let role = self
            .context
            .teams
            .role_of(&mut transaction, team, caller)
            .await?
            .ok_or(UseCaseError::NotFound)?;
        let job = persistence::find(&mut transaction, team, id)
            .await?
            .filter(|job| job.user_id == caller || role == Role::Owner)
            .ok_or(UseCaseError::NotFound)?;
        let skipped_rows = persistence::error_count(&mut transaction, team, id).await?;
        let errors = persistence::errors(&mut transaction, team, id, MAX_ERRORS_SHOWN).await?;
        transaction.commit().await?;
        let errors_truncated = skipped_rows > i64::try_from(errors.len()).unwrap_or(i64::MAX);
        Ok(ImportView {
            id: job.id,
            status: job.status,
            total_rows: job.total_rows,
            imported_rows: job.imported_rows,
            skipped_rows,
            errors,
            errors_truncated,
        })
    }
}
