//! The `receipts` table of Ledgerly-Java's V5 migration.
//!
//! Java counterpart: `receipts/internal/infrastructure/ReceiptRepository.java` and the JPA mapping of
//! `internal/domain/Receipt.java`. Every query carries the team id, also where Java's does not
//! (`existsByExpenseIdAndSha256`, `countByExpenseIdAndScanStatus`, `findByExpenseId`): a receipt id or an
//! expense id alone never reaches a row (OWASP API1).

use ledgerly_shared::id::{ExpenseId, ReceiptId, TeamId, UserId};
use sqlx::PgConnection;

use crate::domain::{DetectedType, Receipt, ScanStatus};

/// The unique index that keeps the same file off one expense twice, even for two uploads at the same moment.
const SAME_FILE_TWICE: &str = "receipts_expense_sha_uk";

/// Why a receipt was not inserted.
#[derive(Debug, thiserror::Error)]
pub(crate) enum InsertError {
    /// The same content is already on the expense: an identical upload won the race.
    #[error("the file is already attached to the expense")]
    Duplicate,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// A new receipt. Java: `ReceiptRepository.saveAndFlush`.
pub(crate) async fn insert(connection: &mut PgConnection, receipt: &Receipt) -> Result<(), InsertError> {
    let inserted = sqlx::query!(
        "insert into receipts (id, team_id, expense_id, storage_key, original_name, detected_type, size_bytes, \
         sha256, scan_status, possible_duplicate, uploaded_by, created_at) \
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        receipt.id.as_uuid(),
        receipt.team_id.as_uuid(),
        receipt.expense_id.as_uuid(),
        receipt.storage_key,
        receipt.original_name,
        receipt.detected_type.mime(),
        receipt.size_bytes,
        receipt.sha256,
        receipt.scan_status.as_str(),
        receipt.possible_duplicate,
        receipt.uploaded_by.as_uuid(),
        receipt.created_at,
    )
    .execute(connection)
    .await;
    match inserted {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(error)) if error.constraint() == Some(SAME_FILE_TWICE) => Err(InsertError::Duplicate),
        Err(error) => Err(error.into()),
    }
}

/// Whether this exact content is already attached to the expense. Java: `existsByExpenseIdAndSha256`.
pub(crate) async fn is_on_expense(
    connection: &mut PgConnection,
    team: TeamId,
    expense: ExpenseId,
    sha256: &str,
) -> sqlx::Result<bool> {
    let found = sqlx::query_scalar!(
        "select exists (select from receipts where team_id = $1 and expense_id = $2 and sha256 = $3) as \"found!\"",
        team.as_uuid(),
        expense.as_uuid(),
        sha256,
    )
    .fetch_one(connection)
    .await?;
    Ok(found)
}

/// Whether the same content is attached to another expense of the team. Java:
/// `existsByTeamIdAndSha256AndExpenseIdNot`.
pub(crate) async fn is_on_another_expense(
    connection: &mut PgConnection,
    team: TeamId,
    expense: ExpenseId,
    sha256: &str,
) -> sqlx::Result<bool> {
    let found = sqlx::query_scalar!(
        "select exists (select from receipts where team_id = $1 and sha256 = $2 and expense_id <> $3) as \"found!\"",
        team.as_uuid(),
        sha256,
        expense.as_uuid(),
    )
    .fetch_one(connection)
    .await?;
    Ok(found)
}

/// The expense's receipts, oldest first. Java: `findByTeamIdAndExpenseIdOrderByCreatedAt`; the id breaks ties
/// between receipts of the same microsecond, where Java's order is the database's.
pub(crate) async fn list(
    connection: &mut PgConnection,
    team: TeamId,
    expense: ExpenseId,
) -> sqlx::Result<Vec<Receipt>> {
    let rows = sqlx::query_as!(
        Row,
        "select id, team_id, expense_id, storage_key, original_name, detected_type, size_bytes, sha256, scan_status, \
         possible_duplicate, uploaded_by, created_at \
         from receipts where team_id = $1 and expense_id = $2 order by created_at, id",
        team.as_uuid(),
        expense.as_uuid(),
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter().map(Receipt::try_from).collect()
}

/// A row as it comes from the table.
struct Row {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    expense_id: uuid::Uuid,
    storage_key: String,
    original_name: String,
    detected_type: String,
    size_bytes: i64,
    sha256: String,
    scan_status: String,
    possible_duplicate: bool,
    uploaded_by: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// A row whose type or status column holds something this module never writes is reported as a decode error.
impl TryFrom<Row> for Receipt {
    type Error = sqlx::Error;

    fn try_from(row: Row) -> sqlx::Result<Self> {
        let decode = |error| sqlx::Error::Decode(Box::new(error));
        Ok(Self {
            id: ReceiptId::from_uuid(row.id),
            team_id: TeamId::from_uuid(row.team_id),
            expense_id: ExpenseId::from_uuid(row.expense_id),
            storage_key: row.storage_key,
            original_name: row.original_name,
            detected_type: row.detected_type.parse::<DetectedType>().map_err(decode)?,
            size_bytes: row.size_bytes,
            sha256: row.sha256,
            scan_status: row.scan_status.parse::<ScanStatus>().map_err(decode)?,
            possible_duplicate: row.possible_duplicate,
            uploaded_by: UserId::from_uuid(row.uploaded_by),
            created_at: row.created_at,
        })
    }
}

/// One receipt of the team. The scan finds its receipt by id, with the team it was uploaded to.
pub(crate) async fn find(connection: &mut PgConnection, team: TeamId, id: ReceiptId) -> sqlx::Result<Option<Receipt>> {
    let row = sqlx::query_as!(
        Row,
        "select id, team_id, expense_id, storage_key, original_name, detected_type, size_bytes, sha256, scan_status, \
         possible_duplicate, uploaded_by, created_at \
         from receipts where team_id = $1 and id = $2",
        team.as_uuid(),
        id.as_uuid(),
    )
    .fetch_optional(connection)
    .await?;
    row.map(Receipt::try_from).transpose()
}

/// `PENDING_SCAN` → `ACCEPTED`. `false` if the receipt is gone or no longer pending.
pub(crate) async fn mark_accepted(connection: &mut PgConnection, team: TeamId, id: ReceiptId) -> sqlx::Result<bool> {
    let updated = sqlx::query!(
        "update receipts set scan_status = 'ACCEPTED' where team_id = $1 and id = $2 and scan_status = 'PENDING_SCAN'",
        team.as_uuid(),
        id.as_uuid(),
    )
    .execute(connection)
    .await?;
    Ok(updated.rows_affected() == 1)
}

/// `PENDING_SCAN` → `QUARANTINED`, with the key the object is moved to. `false` if the receipt is gone or no
/// longer pending.
pub(crate) async fn mark_quarantined(
    connection: &mut PgConnection,
    team: TeamId,
    id: ReceiptId,
    quarantine_key: &str,
) -> sqlx::Result<bool> {
    let updated = sqlx::query!(
        "update receipts set scan_status = 'QUARANTINED', storage_key = $3 \
         where team_id = $1 and id = $2 and scan_status = 'PENDING_SCAN'",
        team.as_uuid(),
        id.as_uuid(),
        quarantine_key,
    )
    .execute(connection)
    .await?;
    Ok(updated.rows_affected() == 1)
}
