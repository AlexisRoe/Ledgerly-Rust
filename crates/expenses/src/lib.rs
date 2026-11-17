//! The expenses module: the `Expense` aggregate and its approval workflow.
//!
//! Java counterpart: `src/main/java/com/ledgerly/expenses/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod application;
mod domain;
mod events;
mod facade;
mod persistence;
mod receipt_checks;
mod web;

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::Limits;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::web::Routes;
use ledgerly_teams::TeamsFacade;
use sqlx::PgPool;

pub use domain::{Category, InvalidInput, NewExpense};
pub use events::{EXPENSE_DELETED, ExpenseEvent, ExpenseEventKind};
pub use facade::{ExpenseRefusal, ExpensesFacade, ValidationProblem};
pub use receipt_checks::{ReceiptChecks, ReceiptSummary, SummaryFuture};

use crate::application::{MembershipLookup, TeamsMembershipLookup};

/// The expense endpoints, ready to be merged into the API's route table.
///
/// Roles come from `teams` on every call. A submit asks `receipts` for the expense's receipts. Every change
/// publishes an [`ExpenseEvent`] through `events`, in its transaction. Lists are capped at
/// `limits.max_page_size`.
#[must_use]
pub fn routes(
    pool: &PgPool,
    clock: &Arc<dyn Clock>,
    limits: &Limits,
    teams: TeamsFacade,
    receipts: &Arc<dyn ReceiptChecks>,
    events: &Arc<dyn Publisher>,
) -> Routes {
    let memberships: Arc<dyn MembershipLookup> = Arc::new(TeamsMembershipLookup(teams));
    web::ExpensesApi::new(pool, clock, &memberships, receipts, events, limits).routes()
}
