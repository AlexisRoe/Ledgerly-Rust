//! The use cases of the expenses module: each opens one transaction, asks the domain, and writes through
//! persistence.
//!
//! Java counterpart: `expenses/internal/application/`. Every change publishes an `ExpenseEvent` in its
//! transaction (M9); the transition metric arrives in M10.

pub(crate) mod access;
mod batch;
mod create_expense;
mod delete_expense;
#[cfg(test)]
pub(crate) mod doubles;
mod export;
mod filters;
mod get_expense;
mod list_expenses;
mod membership;
#[cfg(test)]
mod tests;
mod transition_expense;
mod update_expense;

pub(crate) use {
    batch::{BatchCreateExpenses, Candidate, ItemResult},
    create_expense::{CreateExpense, CreateExpenseCommand},
    delete_expense::DeleteExpense,
    export::{ExportExpenses, ExportPages},
    filters::Filter,
    get_expense::GetExpense,
    list_expenses::{ExpensePage, ListExpenses, ListQuery},
    membership::{MembershipLookup, TeamsMembershipLookup},
    transition_expense::TransitionExpense,
    update_expense::{UpdateExpense, UpdateExpenseCommand},
};

use crate::domain::ExpenseError;
use crate::persistence::PersistenceError;

/// Why a use case did not complete. The web layer maps every variant to a problem response.
#[derive(Debug, thiserror::Error)]
pub(crate) enum UseCaseError {
    /// The caller is not a teammate, or may not see the expense, or it does not exist. Java: `NotFound`.
    #[error("not found")]
    NotFound,
    /// The client's `If-Match` names an older version. Java: `PreconditionFailed`.
    #[error("the expense changed since the version the client read")]
    Stale,
    /// The `Idempotency-Key` was used before with a different request. Java: `Unprocessable(IDEMPOTENCY_KEY_REUSED)`.
    #[error("the idempotency key was used with a different request")]
    IdempotencyKeyReused,
    /// The key's first request is still running, or its expense is gone. Java: `Conflict(IDEMPOTENCY_IN_PROGRESS)`,
    /// with Java's detail.
    #[error("{0}")]
    IdempotencyInProgress(&'static str),
    /// A parameter of the request is not acceptable (page, size, sort, a filter). Java: `InvalidRequest`.
    #[error("{field}: {message}")]
    Invalid { field: &'static str, message: String },
    /// The domain refused: a field rule, a transition, a role. Nothing was written.
    #[error(transparent)]
    Refused(#[from] ExpenseError),
    /// An atomic batch with at least one bad item: nothing was created. Java: `Unprocessable(BATCH_REJECTED)`
    /// with one violation per problem, named `items[<index>].<field>`.
    #[error("nothing was created because some items are invalid")]
    BatchRejected(Vec<ledgerly_shared::web::FieldViolation>),
    /// Fewer ids came back from `create_drafts` than drafts went in: cannot happen.
    #[error("the drafts were not all created")]
    DraftsMissing,
    #[error(transparent)]
    Storage(#[from] PersistenceError),
}

impl UseCaseError {
    pub(crate) fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            message: message.into(),
        }
    }
}

/// Starting or committing the transaction failed.
impl From<sqlx::Error> for UseCaseError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(PersistenceError::Database(error))
    }
}
