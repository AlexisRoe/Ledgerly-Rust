//! The expenses module: the `Expense` aggregate and its approval workflow.
//!
//! Java counterpart: `src/main/java/com/ledgerly/expenses/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod application;
mod domain;
mod persistence;
mod receipt_checks;
mod web;

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::web::Routes;
use sqlx::PgPool;

pub use receipt_checks::{ReceiptChecks, ReceiptSummary};

use crate::application::{MembershipLookup, NoTeamsYet};
use crate::receipt_checks::NoReceiptsYet;

/// The expense endpoints, ready to be merged into the API's route table.
///
/// Until the teams module answers membership questions (M6), nobody is a teammate, so every caller gets 404;
/// until the receipts module exists (M7), no expense has a receipt, so a submit is refused with 422.
#[must_use]
pub fn routes(pool: &PgPool, clock: &Arc<dyn Clock>) -> Routes {
    let memberships: Arc<dyn MembershipLookup> = Arc::new(NoTeamsYet);
    let receipts: Arc<dyn ReceiptChecks> = Arc::new(NoReceiptsYet);
    web::ExpensesApi::new(pool, clock, &memberships, &receipts).routes()
}
