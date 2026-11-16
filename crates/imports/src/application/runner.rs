//! The import job. Java counterpart: `ImportRunner`, `ImportJobConfig` (reader, processor, writer, chunks of
//! `max-batch-size`), `ImportSkipListener` and `ImportJobListener`, on Spring Batch.
//!
//! Each chunk of 100 records is one transaction: its drafts, its row errors and the growth of
//! `imported_rows` commit together or not at all, so progress is visible chunk by chunk and a failure leaves
//! whole chunks behind. Until the outbox (M9.2) the job runs in a task the upload starts after its commit; if
//! the process stops meanwhile, the import stays `QUEUED` or `RUNNING`, as in Java before its registry
//! delivers the event again.

use ledgerly_shared::id::{ImportId, TeamId};
use tokio::io::AsyncReadExt;

use super::Context;
use crate::domain::{self, ImportStatus, Record, Row};
use crate::events::ImportCompleted;
use crate::persistence::{self, Job};

/// Why a run stopped before the end. The import is marked `FAILED`.
#[derive(Debug, thiserror::Error)]
enum RunFailure {
    #[error("database")]
    Database(#[from] sqlx::Error),
    #[error("object storage: {0}")]
    Storage(#[from] ledgerly_shared::storage::StorageError),
    #[error("the file could not be read")]
    Unreadable,
    #[error("creating the drafts failed: {0}")]
    Drafts(String),
    #[error("the committed chunks do not match the file any more")]
    CannotResume,
}

/// One processed record of a chunk.
enum Outcome {
    Ready(ledgerly_expenses::NewExpense),
    Rejected {
        line: u32,
        code: &'static str,
        message: String,
    },
    Blank,
}

/// Runs, and restarts, import jobs.
#[derive(Debug, Clone)]
pub(crate) struct ImportRunner {
    context: Context,
}

impl ImportRunner {
    pub(crate) const fn new(context: Context) -> Self {
        Self { context }
    }

    /// Starts the job in a task of its own and returns at once.
    pub(crate) fn start(&self, team: TeamId, id: ImportId) {
        let runner = self.clone();
        tokio::spawn(async move { runner.run(team, id).await });
    }

    /// Runs the job to its end. Safe to call again for the same import: a `QUEUED` one starts, a `FAILED` one
    /// resumes, a running or finished one is left alone. Java: `ImportRunner.run`.
    pub(crate) async fn run(&self, team: TeamId, id: ImportId) {
        let job = match self.find(team, id).await {
            Ok(Some(job)) => job,
            Ok(None) => return,
            Err(error) => {
                tracing::error!("Import {id} could not be started: {error}");
                return;
            }
        };
        match job.status {
            ImportStatus::Queued => self.execute(job, false).await,
            ImportStatus::Failed => self.execute(job, true).await,
            ImportStatus::Running | ImportStatus::Completed => {
                tracing::info!("Import {id} is {}; nothing to start", job.status.as_str());
            }
        }
    }

    /// Resumes a `FAILED` import after its last committed chunk: rows already imported are not written again.
    /// `false` when there is nothing to restart. Java: `ImportRunner.restart`, for a future admin tool.
    pub(crate) async fn restart(&self, team: TeamId, id: ImportId) -> bool {
        match self.find(team, id).await {
            Ok(Some(job)) if job.status == ImportStatus::Failed => {
                self.execute(job, true).await;
                true
            }
            _ => false,
        }
    }

    async fn find(&self, team: TeamId, id: ImportId) -> sqlx::Result<Option<Job>> {
        let mut connection = self.context.pool.acquire().await?;
        persistence::find(&mut connection, team, id).await
    }

    /// `RUNNING`, the chunks, then `COMPLETED` (and the file deleted) or `FAILED` (the file kept for a
    /// restart), and `ImportCompleted` either way.
    async fn execute(&self, job: Job, resume: bool) {
        let (team, id) = (job.team_id, job.id);
        let outcome = async {
            let mut connection = self.context.pool.acquire().await?;
            persistence::mark_running(&mut connection, team, id, self.context.clock.now()).await?;
            drop(connection);
            self.chunks(&job, resume).await
        }
        .await;
        let status = match &outcome {
            Ok(()) => ImportStatus::Completed,
            Err(failure) => {
                tracing::warn!("Import {id} stopped: {failure}");
                ImportStatus::Failed
            }
        };
        if let Err(error) = self.finish(&job, status).await {
            tracing::error!("Import {id} could not be marked {}: {error}", status.as_str());
        }
        tracing::info!("Import {id} finished: {}", status.as_str());
    }

    async fn chunks(&self, job: &Job, resume: bool) -> Result<(), RunFailure> {
        let mut reader = self.context.storage.open(&job.storage_key).await?;
        let mut content = Vec::new();
        reader
            .read_to_end(&mut content)
            .await
            .map_err(|_| RunFailure::Unreadable)?;
        let text = String::from_utf8_lossy(&content);
        let records = domain::records(&text);
        let chunk_size = usize::try_from(self.context.limits.max_batch_size)
            .unwrap_or(100)
            .max(1);

        // On a restart, the chunks already committed are read again without writing, until their imported and
        // skipped rows add up to what is stored; the next chunk is the first one not committed.
        let (target_imported, target_skipped) = if resume {
            let mut connection = self.context.pool.acquire().await?;
            (
                i64::from(job.imported_rows),
                persistence::error_count(&mut connection, job.team_id, job.id).await?,
            )
        } else {
            (0, 0)
        };
        let mut skipping = target_imported > 0 || target_skipped > 0;
        let (mut imported, mut skipped) = (0_i64, 0_i64);

        for chunk in records.chunks(chunk_size) {
            let outcomes: Vec<Outcome> = chunk.iter().map(|record| self.process(record)).collect();
            if skipping {
                for outcome in &outcomes {
                    match outcome {
                        Outcome::Ready(_) => imported += 1,
                        Outcome::Rejected { .. } => skipped += 1,
                        Outcome::Blank => {}
                    }
                }
                if imported == target_imported && skipped == target_skipped {
                    skipping = false;
                } else if imported > target_imported || skipped > target_skipped {
                    return Err(RunFailure::CannotResume);
                }
                continue;
            }
            self.write(job, outcomes).await?;
        }
        if skipping {
            return Err(RunFailure::CannotResume);
        }
        Ok(())
    }

    /// Java: `RowProcessor.process`, with the expense rules from the expenses module.
    fn process(&self, record: &Record) -> Outcome {
        let rejected = |code, message: String| Outcome::Rejected {
            line: record.line,
            code,
            message,
        };
        match domain::parse_row(&record.fields) {
            Row::Blank => Outcome::Blank,
            Row::Rejected { code, message } => rejected(code, message),
            Row::Ready(expense) => match self.context.drafts.validate(&expense) {
                Some(problem) => rejected("VALIDATION_FAILED", format!("{}: {}", problem.field, problem.message)),
                None => Outcome::Ready(expense),
            },
        }
    }

    /// One chunk, in one transaction: the drafts, the row errors, and the count.
    async fn write(&self, job: &Job, outcomes: Vec<Outcome>) -> Result<(), RunFailure> {
        let mut drafts = Vec::new();
        let mut errors = Vec::new();
        for outcome in outcomes {
            match outcome {
                Outcome::Ready(expense) => drafts.push(expense),
                Outcome::Rejected { line, code, message } => errors.push((line, code, message)),
                Outcome::Blank => {}
            }
        }
        let mut transaction = self.context.pool.begin().await?;
        if !drafts.is_empty() {
            self.context
                .drafts
                .create(&mut transaction, job.team_id, job.user_id, &drafts)
                .await
                .map_err(|refusal| RunFailure::Drafts(refusal.to_string()))?;
            let count = i32::try_from(drafts.len()).unwrap_or(i32::MAX);
            persistence::add_imported(&mut transaction, job.team_id, job.id, count).await?;
        }
        for (line, code, message) in errors {
            let line = i32::try_from(line).unwrap_or(i32::MAX);
            persistence::add_error(&mut transaction, job.id, line, code, &message).await?;
        }
        transaction.commit().await?;
        // Java also counts them (`ledgerly.imports.rows.skipped`); the metrics arrive in M10.3.
        Ok(())
    }

    /// Java: `ImportJobListener.afterJob`.
    async fn finish(&self, job: &Job, status: ImportStatus) -> Result<(), RunFailure> {
        let (team, id) = (job.team_id, job.id);
        let mut transaction = self.context.pool.begin().await?;
        let now = self.context.clock.now();
        persistence::mark_finished(&mut transaction, team, id, status, now).await?;
        let stored = persistence::find(&mut transaction, team, id).await?;
        let skipped_rows = persistence::error_count(&mut transaction, team, id).await?;
        if let Some(stored) = &stored {
            let completed = ImportCompleted {
                event_id: uuid::Uuid::now_v7(),
                team_id: team,
                actor_id: stored.user_id,
                import_id: id,
                imported_rows: i64::from(stored.imported_rows),
                skipped_rows,
                failed: status == ImportStatus::Failed,
                occurred_at: now,
            };
            self.context.events.publish(&mut transaction, &completed).await?;
        }
        transaction.commit().await?;
        // The file is no longer needed; a failed import keeps it so that it can be restarted.
        if status == ImportStatus::Completed
            && let Err(error) = self.context.storage.delete(&job.storage_key).await
        {
            tracing::warn!("Could not delete the file of import {id}: {error}");
        }
        Ok(())
    }
}
