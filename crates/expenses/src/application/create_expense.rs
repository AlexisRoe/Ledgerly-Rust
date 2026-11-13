//! Java counterpart: `expenses/internal/application/CreateExpense.java`.

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::version::Versioned;
use sqlx::PgPool;

use super::UseCaseError;
use crate::domain::{Expense, ExpenseError, NewExpense};
use crate::persistence;

/// What the caller asks for. The claimant is always the caller; nothing in here can set a status or a version.
#[derive(Debug, Clone)]
pub(crate) struct CreateExpenseCommand {
    pub team: TeamId,
    pub claimant: UserId,
    pub expense: NewExpense,
}

/// Creates a draft expense for the caller.
#[derive(Debug, Clone)]
pub(crate) struct CreateExpense {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}

impl CreateExpense {
    pub(crate) fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }

    /// Checks the fields, then stores the new draft at version 0, in one transaction.
    ///
    /// # Errors
    ///
    /// [`UseCaseError::Refused`] with [`ExpenseError::InvalidInput`] naming the first field that breaks a rule
    /// (merchant, note, date in that order), before anything is written; [`UseCaseError::Storage`] if the
    /// database fails.
    pub(crate) async fn execute(&self, command: CreateExpenseCommand) -> Result<Versioned<Expense>, UseCaseError> {
        let expense = Expense::draft(command.team, command.claimant, &command.expense, self.clock.as_ref())
            .map_err(ExpenseError::from)?;
        let mut transaction = self.pool.begin().await?;
        let version = persistence::insert(&mut transaction, &expense, self.clock.now()).await?;
        transaction.commit().await?;
        Ok(Versioned {
            value: expense,
            version,
        })
    }
}
