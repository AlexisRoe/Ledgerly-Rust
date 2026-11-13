//! Stores and loads `Expense` aggregates in the tables of Ledgerly-Java's V1 migration.
//!
//! Java counterpart: `expenses/internal/infrastructure/ExpenseRepository.java` (a bare Spring Data `Repository`,
//! so that no unscoped `findById` exists) and the JPA mapping on `Expense`.
//!
//! Every function that reads by id takes the `TeamId` as well: an expense of another team is "not found", the
//! same answer as one that does not exist (OWASP API1). Writes take the aggregate, whose team cannot change,
//! and use it in the `where` clause too.
//!
//! The functions take a `&mut PgConnection`, which a pool connection and an open transaction (`&mut *tx`) both
//! deref to, so the caller decides the transaction boundary.

#[cfg(test)]
mod tests;

use chrono::{DateTime, NaiveDate, Utc};
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::money::{Currency, Money};
use ledgerly_shared::version::{Version, Versioned};
use sqlx::PgConnection;
use sqlx::types::{Decimal, Uuid};

use crate::domain::{Category, Expense, ExpenseStatus, Merchant, Note, Rejection, RejectionReason, StoredExpense};

/// Why a persistence call failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PersistenceError {
    /// The expense was changed or deleted since the version the caller holds. Java:
    /// `ObjectOptimisticLockingFailureException`.
    #[error("the expense was changed or deleted since version {expected}")]
    Conflict { expected: Version },
    /// A stored row breaks a rule the domain guarantees. Nothing Ledgerly writes can cause this; a hand-made
    /// change to the database can. The reason names the column, never its value.
    #[error("the stored expense {id} cannot be read: {reason}")]
    Corrupt { id: Uuid, reason: String },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The expense with this id in this team, with its rejections oldest first, or `None` if there is none in
/// that team.
pub(crate) async fn find(
    connection: &mut PgConnection,
    team: TeamId,
    id: ExpenseId,
) -> Result<Option<Versioned<Expense>>, PersistenceError> {
    // One statement, so the expense and its rejections come from the same snapshot. The arrays are ordered
    // by `position`, Hibernate's `@OrderColumn`.
    let row = sqlx::query_as!(
        ExpenseRow,
        r#"
        select e.id, e.team_id, e.claimant_id, e.amount, e.currency, e.category, e.incurred_on, e.merchant,
               e.note, e.status, e.version,
               coalesce(array_agg(r.rejected_by order by r.position) filter (where r.expense_id is not null),
                        '{}') as "rejected_by!",
               coalesce(array_agg(r.reason order by r.position) filter (where r.expense_id is not null),
                        '{}') as "rejection_reasons!"
          from expenses e
          left join expense_rejections r on r.expense_id = e.id
         where e.team_id = $1 and e.id = $2
         group by e.id
        "#,
        team.as_uuid(),
        id.as_uuid(),
    )
    .fetch_optional(connection)
    .await?;
    row.map(Versioned::try_from).transpose()
}

/// Inserts a new expense at version 0, with `created_at` and `updated_at` both `now`.
pub(crate) async fn insert(
    connection: &mut PgConnection,
    expense: &Expense,
    now: DateTime<Utc>,
) -> Result<Version, PersistenceError> {
    let version = Version::INITIAL;
    sqlx::query!(
        "insert into expenses (id, team_id, claimant_id, amount, currency, category, incurred_on, merchant, note,
                               status, version, created_at, updated_at)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $12)",
        expense.id().as_uuid(),
        expense.team_id().as_uuid(),
        expense.claimant_id().as_uuid(),
        expense.money().amount(),
        expense.money().currency().code(),
        expense.category().as_str(),
        expense.incurred_on(),
        expense.merchant().as_str(),
        expense.note().map(Note::as_str),
        expense.status().as_str(),
        version.get(),
        now,
    )
    .execute(&mut *connection)
    .await?;
    insert_rejections(connection, expense, 0).await?;
    Ok(version)
}

/// Writes the expense's current state if the stored row is still at `expected`, and returns the new version.
///
/// Rejections are only ever added, so the ones beyond those already stored are inserted.
///
/// # Errors
///
/// [`PersistenceError::Conflict`] if the row has a different version, was deleted, or is not in the
/// expense's team.
pub(crate) async fn update(
    connection: &mut PgConnection,
    expense: &Expense,
    expected: Version,
    now: DateTime<Utc>,
) -> Result<Version, PersistenceError> {
    let next = expected.next().ok_or(PersistenceError::Conflict { expected })?;
    let updated = sqlx::query!(
        "update expenses
            set amount = $3, currency = $4, category = $5, incurred_on = $6, merchant = $7, note = $8,
                status = $9, version = $10, updated_at = $11
          where team_id = $1 and id = $2 and version = $12",
        expense.team_id().as_uuid(),
        expense.id().as_uuid(),
        expense.money().amount(),
        expense.money().currency().code(),
        expense.category().as_str(),
        expense.incurred_on(),
        expense.merchant().as_str(),
        expense.note().map(Note::as_str),
        expense.status().as_str(),
        next.get(),
        now,
        expected.get(),
    )
    .execute(&mut *connection)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(PersistenceError::Conflict { expected });
    }
    // The update above holds the row lock until the transaction ends, so nobody adds rejections in between.
    let stored = sqlx::query_scalar!(
        r#"select count(*) as "count!" from expense_rejections where expense_id = $1"#,
        expense.id().as_uuid(),
    )
    .fetch_one(&mut *connection)
    .await?;
    insert_rejections(connection, expense, stored).await?;
    Ok(next)
}

/// Deletes the expense, and with it its rejections (`on delete cascade`), if it is still at `expected`.
///
/// Java deletes a loaded entity, and Hibernate checks its version in the `delete` statement too.
///
/// # Errors
///
/// [`PersistenceError::Conflict`] if there is no such expense in the team at that version.
pub(crate) async fn delete(
    connection: &mut PgConnection,
    team: TeamId,
    id: ExpenseId,
    expected: Version,
) -> Result<(), PersistenceError> {
    let deleted = sqlx::query!(
        "delete from expenses where team_id = $1 and id = $2 and version = $3",
        team.as_uuid(),
        id.as_uuid(),
        expected.get(),
    )
    .execute(connection)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(PersistenceError::Conflict { expected });
    }
    Ok(())
}

/// Inserts the rejections from position `already_stored` on, at their index in the aggregate's list.
async fn insert_rejections(
    connection: &mut PgConnection,
    expense: &Expense,
    already_stored: i64,
) -> Result<(), PersistenceError> {
    let corrupt = |reason: &str| PersistenceError::Corrupt {
        id: expense.id().as_uuid(),
        reason: reason.to_owned(),
    };
    let skip = usize::try_from(already_stored).map_err(|_| corrupt("negative rejection count"))?;
    for (position, rejection) in expense.rejections().iter().enumerate().skip(skip) {
        let position = i32::try_from(position).map_err(|_| corrupt("more rejections than an integer holds"))?;
        sqlx::query!(
            "insert into expense_rejections (expense_id, position, rejected_by, reason) values ($1, $2, $3, $4)",
            expense.id().as_uuid(),
            position,
            rejection.rejected_by().as_uuid(),
            rejection.reason().as_str(),
        )
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

/// One row of `expenses` with its rejections, as [`find`] selects it.
struct ExpenseRow {
    id: Uuid,
    team_id: Uuid,
    claimant_id: Uuid,
    amount: Decimal,
    currency: String,
    category: String,
    incurred_on: NaiveDate,
    merchant: String,
    note: Option<String>,
    status: String,
    version: i64,
    rejected_by: Vec<Uuid>,
    rejection_reasons: Vec<String>,
}

impl TryFrom<ExpenseRow> for Versioned<Expense> {
    type Error = PersistenceError;

    /// Every column goes through the same constructor the domain uses, so a row the domain could not have
    /// written is reported, not loaded.
    fn try_from(row: ExpenseRow) -> Result<Self, Self::Error> {
        let corrupt = |column: &str| PersistenceError::Corrupt {
            id: row.id,
            reason: format!("invalid {column}"),
        };
        let currency = Currency::from_code(&row.currency).map_err(|_| corrupt("currency"))?;
        let money = Money::new(row.amount, currency).map_err(|_| corrupt("amount"))?;
        if row.rejected_by.len() != row.rejection_reasons.len() {
            return Err(corrupt("rejections"));
        }
        let rejections = row
            .rejected_by
            .iter()
            .zip(&row.rejection_reasons)
            .map(|(rejected_by, reason)| {
                RejectionReason::new(reason)
                    .map(|reason| Rejection::restore(UserId::from_uuid(*rejected_by), reason))
                    .map_err(|_| corrupt("rejection reason"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let stored = StoredExpense {
            id: ExpenseId::from_uuid(row.id),
            team_id: TeamId::from_uuid(row.team_id),
            claimant_id: UserId::from_uuid(row.claimant_id),
            money,
            category: row.category.parse::<Category>().map_err(|_| corrupt("category"))?,
            incurred_on: row.incurred_on,
            merchant: Merchant::new(&row.merchant).map_err(|_| corrupt("merchant"))?,
            note: row
                .note
                .as_deref()
                .map(Note::new)
                .transpose()
                .map_err(|_| corrupt("note"))?,
            status: row.status.parse::<ExpenseStatus>().map_err(|_| corrupt("status"))?,
            rejections,
        };
        Ok(Self {
            value: Expense::restore(stored),
            version: Version::from_stored(row.version),
        })
    }
}
