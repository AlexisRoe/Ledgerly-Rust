//! Java counterpart: `imports/internal/application/UploadImport.java`.

use chrono::{DateTime, Duration, NaiveTime, Utc};
use ledgerly_shared::id::{ImportId, TeamId, UserId};
use ledgerly_shared::text::display_name;
use ledgerly_shared::web::MultipartUpload;
use sqlx::PgConnection;
use tokio::io::AsyncWriteExt;

use super::{Context, ImportRunner, UseCaseError};
use crate::domain::{self, ImportStatus};
use crate::persistence::{self, Job};

const MEBIBYTE: u64 = 1024 * 1024;

/// Accepts a CSV for import. Everything that can be refused cheaply is refused before a job exists: not a
/// teammate, the daily limit used up, too big, not text, the wrong header, too many rows. Only then is the
/// file stored and a `QUEUED` import saved; the job starts once that has committed.
#[derive(Debug, Clone)]
pub(crate) struct UploadImport {
    context: Context,
    runner: ImportRunner,
}

impl UploadImport {
    pub(crate) const fn new(context: Context, runner: ImportRunner) -> Self {
        Self { context, runner }
    }

    /// The new import's id.
    ///
    /// Nothing of the body is read before the teammate check and the daily limit, as for receipts (the
    /// reviewer's decision after M7); Spring reads the whole request before Java's controller runs.
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        caller: UserId,
        upload: MultipartUpload,
    ) -> Result<ImportId, UseCaseError> {
        {
            let mut connection = self.context.pool.acquire().await?;
            self.context
                .teams
                .role_of(&mut connection, team, caller)
                .await?
                .ok_or(UseCaseError::NotFound)?;
            // A quick refusal before reading the upload; the deciding check runs under the lock below.
            self.require_under_daily_limit(&mut connection, team, caller).await?;
        }

        let mut file = upload.file("file").await.map_err(UseCaseError::Upload)?;
        let original_name = display_name(file.original_name(), "import.csv");
        let limit = self.context.limits.max_upload_size;
        let mut content = Vec::new();
        let mut over_the_limit = false;
        // Over the cap, the rest is still read, so that Spring's own limit speaks first (as for receipts).
        while let Some(chunk) = file.chunk().await.map_err(UseCaseError::Upload)? {
            if over_the_limit || content.len() as u64 + chunk.len() as u64 > limit {
                over_the_limit = true;
                continue;
            }
            content.extend_from_slice(&chunk);
        }
        if over_the_limit {
            return Err(UseCaseError::TooLarge(limit / MEBIBYTE));
        }
        let rows = domain::inspect(&content, self.context.limits.max_import_rows).map_err(UseCaseError::Refused)?;

        let key = format!("imports/{}", uuid::Uuid::new_v4());
        self.store(&key, &content).await?;
        let job = Job {
            id: ImportId::new_v4(),
            team_id: team,
            user_id: caller,
            status: ImportStatus::Queued,
            original_name,
            storage_key: key,
            total_rows: i32::try_from(rows).unwrap_or(i32::MAX),
            imported_rows: 0,
            created_at: self.context.clock.now(),
        };
        if let Err(error) = self.record(&job).await {
            if let Err(cleanup) = self.context.storage.delete(&job.storage_key).await {
                tracing::warn!("Could not remove the file of a refused import: {cleanup}");
            }
            return Err(error);
        }
        self.runner.start(team, job.id);
        Ok(job.id)
    }

    async fn store(&self, key: &str, content: &[u8]) -> Result<(), UseCaseError> {
        let staged = tempfile::Builder::new()
            .prefix("import-")
            .suffix(".csv")
            .tempfile()
            .map_err(UseCaseError::Staging)?;
        let mut writer = tokio::fs::File::from_std(staged.reopen().map_err(UseCaseError::Staging)?);
        writer.write_all(content).await.map_err(UseCaseError::Staging)?;
        writer.flush().await.map_err(UseCaseError::Staging)?;
        self.context.storage.put(key, staged.path(), "text/csv").await?;
        Ok(())
    }

    /// Under the per-user lock: the deciding daily-limit check, then the `QUEUED` row.
    async fn record(&self, job: &Job) -> Result<(), UseCaseError> {
        let mut transaction = self.context.pool.begin().await?;
        persistence::lock_user_in_team(&mut transaction, job.team_id, job.user_id).await?;
        self.require_under_daily_limit(&mut transaction, job.team_id, job.user_id)
            .await?;
        persistence::insert(&mut transaction, job).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// At most `max-imports-per-day` per user and team per UTC day; "today" comes from the clock. A refusal
    /// carries the seconds until UTC midnight (at least 1).
    async fn require_under_daily_limit(
        &self,
        connection: &mut PgConnection,
        team: TeamId,
        user: UserId,
    ) -> Result<(), UseCaseError> {
        let now = self.context.clock.now();
        let start_of_day = start_of_utc_day(now);
        let limit = self.context.limits.max_imports_per_day;
        if persistence::count_since(connection, team, user, start_of_day).await? >= i64::from(limit) {
            let next_midnight = start_of_day + Duration::days(1);
            return Err(UseCaseError::DailyLimit {
                limit,
                retry_after_seconds: (next_midnight - now).num_seconds().max(1),
            });
        }
        Ok(())
    }
}

fn start_of_utc_day(now: DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive().and_time(NaiveTime::MIN).and_utc()
}
