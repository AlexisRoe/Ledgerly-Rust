//! The tables of Ledgerly-Java's V6 migration that Ledgerly owns: `import_jobs` and `import_row_errors`.
//! Spring Batch's `BATCH_*` tables are not used (plan M8.4).
//!
//! Java counterpart: `imports/internal/infrastructure/ImportJobs.java`. Every query that reads an import carries
//! its team, also where Java's (`find`, `errorCount`, `errors`) does not (OWASP API1).

use chrono::{DateTime, Utc};
use ledgerly_shared::id::{ImportId, TeamId, UserId};
use ledgerly_shared::text::truncate_utf16;
use sqlx::PgConnection;

use crate::domain::ImportStatus;

/// One row of `import_jobs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Job {
    pub id: ImportId,
    pub team_id: TeamId,
    pub user_id: UserId,
    pub status: ImportStatus,
    pub original_name: String,
    pub storage_key: String,
    pub total_rows: i32,
    pub imported_rows: i32,
    pub created_at: DateTime<Utc>,
}

/// A rejected line: its number in the file, a stable code, and a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowError {
    pub line: i32,
    pub code: String,
    pub message: String,
}

/// Serialises the uploads of one user in one team until the transaction ends, so that "count today's imports,
/// then insert" cannot be raced past the daily limit. The same key as Java's, so both take the same lock.
pub(crate) async fn lock_user_in_team(connection: &mut PgConnection, team: TeamId, user: UserId) -> sqlx::Result<()> {
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("import:{team}:{user}"))
        .execute(connection)
        .await?;
    Ok(())
}

/// How many imports the user started in the team since `since`.
pub(crate) async fn count_since(
    connection: &mut PgConnection,
    team: TeamId,
    user: UserId,
    since: DateTime<Utc>,
) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        "select count(*) as \"count!\" from import_jobs where team_id = $1 and user_id = $2 and created_at >= $3",
        team.as_uuid(),
        user.as_uuid(),
        since,
    )
    .fetch_one(connection)
    .await
}

/// A new import, with no row imported yet.
pub(crate) async fn insert(connection: &mut PgConnection, job: &Job) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into import_jobs (id, team_id, user_id, status, original_name, storage_key, total_rows, \
         imported_rows, created_at) values ($1, $2, $3, $4, $5, $6, $7, 0, $8)",
        job.id.as_uuid(),
        job.team_id.as_uuid(),
        job.user_id.as_uuid(),
        job.status.as_str(),
        job.original_name,
        job.storage_key,
        job.total_rows,
        job.created_at,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// The import of the team, if there is one.
pub(crate) async fn find(connection: &mut PgConnection, team: TeamId, id: ImportId) -> sqlx::Result<Option<Job>> {
    let row = sqlx::query!(
        "select id, team_id, user_id, status, original_name, storage_key, total_rows, imported_rows, created_at \
         from import_jobs where team_id = $1 and id = $2",
        team.as_uuid(),
        id.as_uuid(),
    )
    .fetch_optional(connection)
    .await?;
    row.map(|row| {
        let status = ImportStatus::parse(&row.status)
            .ok_or_else(|| sqlx::Error::Decode(format!("unknown import status {}", row.status).into()))?;
        Ok(Job {
            id: ImportId::from_uuid(row.id),
            team_id: TeamId::from_uuid(row.team_id),
            user_id: UserId::from_uuid(row.user_id),
            status,
            original_name: row.original_name,
            storage_key: row.storage_key,
            total_rows: row.total_rows,
            imported_rows: row.imported_rows,
            created_at: row.created_at,
        })
    })
    .transpose()
}

/// `RUNNING`, with the start time of the first run kept, and no finish time.
pub(crate) async fn mark_running(
    connection: &mut PgConnection,
    team: TeamId,
    id: ImportId,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "update import_jobs set status = 'RUNNING', started_at = coalesce(started_at, $3), finished_at = null \
         where team_id = $1 and id = $2",
        team.as_uuid(),
        id.as_uuid(),
        now,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// `COMPLETED` or `FAILED`, with the finish time.
pub(crate) async fn mark_finished(
    connection: &mut PgConnection,
    team: TeamId,
    id: ImportId,
    status: ImportStatus,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "update import_jobs set status = $3, finished_at = $4 where team_id = $1 and id = $2",
        team.as_uuid(),
        id.as_uuid(),
        status.as_str(),
        now,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Called in the chunk's own transaction, so a chunk that rolls back is not counted.
pub(crate) async fn add_imported(
    connection: &mut PgConnection,
    team: TeamId,
    id: ImportId,
    rows: i32,
) -> sqlx::Result<()> {
    sqlx::query!(
        "update import_jobs set imported_rows = imported_rows + $3 where team_id = $1 and id = $2",
        team.as_uuid(),
        id.as_uuid(),
        rows,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Records a rejected line; the same line twice is ignored (a restarted import may report it again). The
/// message is cut at 500 characters, as the column is.
pub(crate) async fn add_error(
    connection: &mut PgConnection,
    id: ImportId,
    line: i32,
    code: &str,
    message: &str,
) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into import_row_errors (import_id, line, code, message) values ($1, $2, $3, $4) \
         on conflict do nothing",
        id.as_uuid(),
        line,
        code,
        truncate_utf16(message, 500),
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// How many lines of the team's import were rejected.
pub(crate) async fn error_count(connection: &mut PgConnection, team: TeamId, id: ImportId) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        "select count(*) as \"count!\" from import_row_errors e join import_jobs j on j.id = e.import_id \
         where j.team_id = $1 and e.import_id = $2",
        team.as_uuid(),
        id.as_uuid(),
    )
    .fetch_one(connection)
    .await
}

/// The first `limit` rejected lines, by line number.
pub(crate) async fn errors(
    connection: &mut PgConnection,
    team: TeamId,
    id: ImportId,
    limit: i64,
) -> sqlx::Result<Vec<RowError>> {
    let rows = sqlx::query!(
        "select e.line, e.code, e.message from import_row_errors e join import_jobs j on j.id = e.import_id \
         where j.team_id = $1 and e.import_id = $2 order by e.line limit $3",
        team.as_uuid(),
        id.as_uuid(),
        limit,
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| RowError {
            line: row.line,
            code: row.code,
            message: row.message,
        })
        .collect())
}
