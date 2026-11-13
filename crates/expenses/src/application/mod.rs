//! The use cases of the expenses module: each opens one transaction, asks the domain, and writes through
//! persistence.
//!
//! Java counterpart: `expenses/internal/application/`. What Java's use cases also do arrives with later steps:
//! the membership check through `MembershipLookup` (M3.3), the `Idempotency-Key` (M3.5) and events (M5.7, M9).

mod create_expense;
#[cfg(test)]
pub(crate) mod doubles;
mod get_expense;
mod membership;
#[cfg(test)]
mod tests;

#[cfg_attr(not(test), expect(unused_imports, reason = "the web layer imports them from M3 on"))]
pub(crate) use {
    create_expense::{CreateExpense, CreateExpenseCommand},
    get_expense::GetExpense,
    membership::{MembershipLookup, NoTeamsYet},
};

use crate::domain::ExpenseError;
use crate::persistence::PersistenceError;

/// Why a use case did not complete. The web layer (M3) maps every variant to a problem response.
#[derive(Debug, thiserror::Error)]
pub(crate) enum UseCaseError {
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
