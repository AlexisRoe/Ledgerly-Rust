//! The `audit_events` table. Java counterpart: `audit/internal/infrastructure/AuditTrail.java`.
//!
//! Deliberately only [`append`] and [`page`]: nothing here updates or deletes a row, and the database refuses
//! it as well (V8: privileges revoked from everyone, and triggers that raise on `UPDATE`, `DELETE` and
//! `TRUNCATE`).

use chrono::{DateTime, Utc};
use ledgerly_shared::id::TeamId;
use ledgerly_shared::paging::{self, Cursor, KeysetPage};
use sqlx::PgConnection;
use sqlx::types::Json;
use uuid::Uuid;

use crate::details::Details;

/// What is stored for one event: ids and field names only.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub event_id: Uuid,
    /// `None` for events that belong to no team (registering, verifying).
    pub team_id: Option<Uuid>,
    /// `None` when the system acted (the malware scan).
    pub actor_id: Option<Uuid>,
    pub event_type: &'static str,
    pub subject_type: &'static str,
    pub subject_id: Uuid,
    pub details: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

/// A stored row as read back. `created_at` is when it was recorded, `occurred_at` when it happened.
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct Row {
    pub id: Uuid,
    pub actor_id: Option<Uuid>,
    pub event_type: String,
    pub subject_type: Option<String>,
    pub subject_id: Option<Uuid>,
    pub details: Json<Details>,
    pub occurred_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Records `entry`, written at `now`. Idempotent on the event id: an event already recorded is left as it is.
pub(crate) async fn append(connection: &mut PgConnection, entry: &Entry, now: DateTime<Utc>) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into audit_events (id, event_id, team_id, actor_id, event_type, subject_type, subject_id, details,
                                   occurred_at, created_at)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         on conflict (event_id) do nothing",
        Uuid::now_v7(),
        entry.event_id,
        entry.team_id,
        entry.actor_id,
        entry.event_type,
        entry.subject_type,
        entry.subject_id,
        &entry.details,
        entry.occurred_at,
        now,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// One team's trail, newest written first. Rows without a team are never part of it.
pub(crate) async fn page(
    connection: &mut PgConnection,
    team: TeamId,
    after: Option<Cursor>,
    limit: u32,
) -> sqlx::Result<KeysetPage<Row>> {
    paging::fetch(connection, "audit_events", team, after, limit, |row: &Row| {
        Cursor::new(row.created_at, row.id)
    })
    .await
}
