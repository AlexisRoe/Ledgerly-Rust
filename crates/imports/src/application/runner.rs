//! The import job. Java counterpart: `ImportRunner`, `ImportJobConfig` (reader, processor, writer, chunks of
//! `max-batch-size`), `ImportSkipListener` and `ImportJobListener`, on Spring Batch.
//!
//! Each chunk of 100 records is one transaction: its drafts, its row errors and the growth of
//! `imported_rows` commit together or not at all, so progress is visible chunk by chunk and a failure leaves
//! whole chunks behind.
//!
//! The job runs when the outbox delivers `ImportQueued` ([`ImportQueuedListener`]), and the publication stays
//! open until the job has ended. If the process stops meanwhile, the next start delivers it again: a `QUEUED`
//! import starts, and one left `RUNNING` with no live runner resumes after its last committed chunk (decided
//! after M8; Java leaves it `RUNNING`).

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use ledgerly_shared::events::{Delivery, DeliveryFailed, Listener, read_payload};
use ledgerly_shared::id::{ImportId, TeamId};
use tokio::io::AsyncReadExt;

use super::Context;
use crate::domain::{self, ImportStatus, Record, Row};
use crate::events::{IMPORT_QUEUED, ImportCompleted, ImportQueued};
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
    /// The imports whose job runs in this process right now.
    live: Arc<Mutex<HashSet<ImportId>>>,
}

/// Takes an import off the live set when its job ends, also when it panicked.
struct Live {
    set: Arc<Mutex<HashSet<ImportId>>>,
    id: ImportId,
}

impl Drop for Live {
    fn drop(&mut self) {
        self.set.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.id);
    }
}

impl ImportRunner {
    pub(crate) fn new(context: Context) -> Self {
        Self {
            context,
            live: Arc::default(),
        }
    }

    /// Runs the job to its end. Safe to call again for the same import: a `QUEUED` one starts, a `FAILED` one
    /// resumes, a `RUNNING` one resumes unless its job runs in this process, a finished one is left alone.
    /// Java: `ImportRunner.run`, which leaves a `RUNNING` one alone.
    ///
    /// # Errors
    ///
    /// The database error if the import cannot be read; the job's own failures end as `FAILED` instead.
    pub(crate) async fn run(&self, team: TeamId, id: ImportId) -> sqlx::Result<()> {
        let Some(job) = self.find(team, id).await? else {
            return Ok(());
        };
        match job.status {
            ImportStatus::Queued => self.execute(job, false).await,
            ImportStatus::Failed | ImportStatus::Running => self.execute(job, true).await,
            ImportStatus::Completed => {
                tracing::info!("Import {id} is {}; nothing to start", job.status.as_str());
            }
        }
        Ok(())
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
        if !self.live.lock().unwrap_or_else(PoisonError::into_inner).insert(id) {
            tracing::info!("Import {id} is running; nothing to start");
            return;
        }
        let _live = Live {
            set: self.live.clone(),
            id,
        };
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

/// Runs the job of a queued import. Java: `ImportRunner.on(ImportQueued)`, a tracked `@Async
/// @TransactionalEventListener`, not an `@ApplicationModuleListener`, so that the job is not one big
/// transaction. The delivery ends with the job, `COMPLETED` or `FAILED` alike; it fails only if the import
/// could not even be read, and is then delivered again.
#[derive(Debug, Clone)]
pub(crate) struct ImportQueuedListener(pub ImportRunner);

impl Listener for ImportQueuedListener {
    fn id(&self) -> &'static str {
        "ledgerly-rust:imports.ImportRunner"
    }

    fn listens_to(&self, event_type: &str) -> bool {
        event_type == IMPORT_QUEUED
    }

    fn deliver<'a>(&'a self, event_type: &'a str, payload: &'a str) -> Delivery<'a> {
        Box::pin(async move {
            let queued: ImportQueued = read_payload(event_type, payload)?;
            self.0
                .run(queued.team_id, queued.import_id)
                .await
                .map_err(|_| DeliveryFailed(format!("import {} could not be read: database", queued.import_id)))
        })
    }
}
