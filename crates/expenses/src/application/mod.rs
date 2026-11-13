//! The use cases of the expenses module: each opens one transaction, asks the domain, and writes through
//! persistence.
//!
//! Java counterpart: `expenses/internal/application/`. What Java's use cases also do arrives with later steps:
//! the `Idempotency-Key` (M3.5), events (M9) and the transition metric (M10).

mod access;
mod create_expense;
mod delete_expense;
#[cfg(test)]
pub(crate) mod doubles;
mod get_expense;
mod membership;
#[cfg(test)]
mod tests;
mod transition_expense;
mod update_expense;

pub(crate) use {
    create_expense::{CreateExpense, CreateExpenseCommand},
    delete_expense::DeleteExpense,
    get_expense::GetExpense,
    membership::{MembershipLookup, NoTeamsYet},
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
    /// The domain refused: a field rule, a transition, a role. Nothing was written.
    #[error(transparent)]
    Refused(#[from] ExpenseError),
    #[error(transparent)]
    Storage(#[from] PersistenceError),
}

/// Starting or committing the transaction failed.
impl From<sqlx::Error> for UseCaseError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(PersistenceError::Database(error))
    }
}
