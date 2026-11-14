//! Java counterpart: `expenses/internal/application/TransitionExpense.java`.

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::version::Versioned;
use sqlx::PgPool;

use super::{MembershipLookup, UseCaseError, access};
use crate::ReceiptChecks;
use crate::domain::{Action, Expense};
use crate::persistence;

/// The six workflow transitions. Transitions take no `If-Match`, as in Java; two at once still cannot both
/// win, because the write is version-checked.
#[derive(Debug, Clone)]
pub(crate) struct TransitionExpense {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    memberships: Arc<dyn MembershipLookup>,
    receipts: Arc<dyn ReceiptChecks>,
}

impl TransitionExpense {
    pub(crate) fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        memberships: Arc<dyn MembershipLookup>,
        receipts: Arc<dyn ReceiptChecks>,
    ) -> Self {
        Self {
            pool,
            clock,
            memberships,
            receipts,
        }
    }

    /// Applies `action`. `reason` is read by [`Action::Reject`] only.
    ///
    /// # Errors
    ///
    /// Not visible (404), then the domain's refusals in its order, or a storage failure; a concurrent change
    /// makes the write a `Conflict` (412).
    pub(crate) async fn execute(
        &self,
        action: Action,
        team: TeamId,
        id: ExpenseId,
        caller: UserId,
        reason: Option<&str>,
    ) -> Result<Versioned<Expense>, UseCaseError> {
        let mut transaction = self.pool.begin().await?;
        let actor = access::actor(&mut transaction, self.memberships.as_ref(), team, caller).await?;
        let found = access::load(&mut transaction, actor, team, id).await?;
        let mut expense = found.value;
        match action {
            Action::Submit => expense.submit(actor, self.receipts.summary_for(team, id))?,
            Action::Withdraw => expense.withdraw(actor)?,
            Action::Approve => expense.approve(actor)?,
            Action::Reject => expense.reject(actor, reason.unwrap_or_default())?,
            Action::Reopen => expense.reopen(actor)?,
            Action::Reimburse => expense.reimburse(actor)?,
        }
        let version = persistence::update(&mut transaction, &expense, found.version, self.clock.now()).await?;
        transaction.commit().await?;
        Ok(Versioned {
            value: expense,
            version,
        })
    }
}
