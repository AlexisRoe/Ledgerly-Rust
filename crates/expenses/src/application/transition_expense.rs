//! Java counterpart: `expenses/internal/application/TransitionExpense.java`.

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::version::Versioned;
use sqlx::PgPool;

use super::{MembershipLookup, UseCaseError, access};
use crate::ReceiptChecks;
use crate::domain::{Action, Expense};
use crate::events::{ExpenseEvent, ExpenseEventKind};
use crate::persistence;

/// The six workflow transitions. Transitions take no `If-Match`, as in Java; two at once still cannot both
/// win, because the write is version-checked.
#[derive(Debug, Clone)]
pub(crate) struct TransitionExpense {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    memberships: Arc<dyn MembershipLookup>,
    receipts: Arc<dyn ReceiptChecks>,
    events: Arc<dyn Publisher>,
}

impl TransitionExpense {
    pub(crate) fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        memberships: Arc<dyn MembershipLookup>,
        receipts: Arc<dyn ReceiptChecks>,
        events: Arc<dyn Publisher>,
    ) -> Self {
        Self {
            pool,
            clock,
            memberships,
            receipts,
            events,
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
            Action::Submit => {
                let receipts = self.receipts.summary_for(&mut transaction, team, id).await?;
                expense.submit(actor, receipts)?;
            }
            Action::Withdraw => expense.withdraw(actor)?,
            Action::Approve => expense.approve(actor)?,
            Action::Reject => expense.reject(actor, reason.unwrap_or_default())?,
            Action::Reopen => expense.reopen(actor)?,
            Action::Reimburse => expense.reimburse(actor)?,
        }
        let version = persistence::update(&mut transaction, &expense, found.version, self.clock.now()).await?;
        // In the same transaction as the change: either both happen or neither does.
        let kind = match action {
            Action::Submit => ExpenseEventKind::Submitted,
            Action::Withdraw => ExpenseEventKind::Withdrawn,
            Action::Approve => ExpenseEventKind::Approved,
            Action::Reject => ExpenseEventKind::Rejected,
            Action::Reopen => ExpenseEventKind::Reopened,
            Action::Reimburse => ExpenseEventKind::Reimbursed,
        };
        let event = ExpenseEvent::new(team, id, caller, self.clock.now(), kind);
        self.events.publish(&mut transaction, &event).await?;
        // Counted per action. Only transitions that succeeded get here: a refusal returned above. Java counts
        // before its transaction commits; a failed commit is counted there and not here.
        transaction.commit().await?;
        metrics::counter!("ledgerly.expenses.transitions", "action" => action.metric_tag()).increment(1);
        Ok(Versioned {
            value: expense,
            version,
        })
    }
}
