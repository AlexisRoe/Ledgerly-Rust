//! The expenses module: the `Expense` aggregate and its approval workflow.
//!
//! Java counterpart: `src/main/java/com/ledgerly/expenses/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod application;
mod domain;
mod facade;
mod persistence;
mod receipt_checks;
mod web;

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::Limits;
use ledgerly_shared::web::Routes;
use ledgerly_teams::TeamsFacade;
use sqlx::PgPool;

pub use facade::{ExpenseRefusal, ExpensesFacade};
pub use receipt_checks::{ReceiptChecks, ReceiptSummary};

use crate::application::{MembershipLookup, TeamsMembershipLookup};
use crate::receipt_checks::NoReceiptsYet;

/// The expense endpoints, ready to be merged into the API's route table.
///
/// Roles come from `teams` on every call. Until the receipts module exists (M7), no expense has a receipt, so a
/// submit is refused with 422. Lists are capped at `limits.max_page_size`.
#[must_use]
pub fn routes(pool: &PgPool, clock: &Arc<dyn Clock>, limits: &Limits, teams: TeamsFacade) -> Routes {
    let memberships: Arc<dyn MembershipLookup> = Arc::new(TeamsMembershipLookup(teams));
    let receipts: Arc<dyn ReceiptChecks> = Arc::new(NoReceiptsYet);
    web::ExpensesApi::new(pool, clock, &memberships, &receipts, limits.max_page_size).routes()
}
