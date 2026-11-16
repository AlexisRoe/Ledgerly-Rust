//! Who is told that an expense was deleted.
//!
//! Java counterpart: the listeners of `ExpenseEvent.Deleted`, of which receipts' `ExpenseDeletedCleanup` is the
//! one that acts. Expenses owns the interface and receipts implements it, like [`crate::ReceiptChecks`], so the
//! crates form no cycle.
//!
//! Until the outbox (M9.2) it is called once the deletion has committed, and a crash in between leaves the
//! expense's receipts behind; Java's event registry delivers the event again. M9.2 moves it onto the outbox.

use std::fmt;

use ledgerly_shared::id::{ExpenseId, TeamId};

/// Told after an expense of `team` was deleted. It must return at once; the work happens in a task of its own.
pub trait ExpenseDeletedListener: fmt::Debug + Send + Sync {
    fn expense_deleted(&self, team: TeamId, expense: ExpenseId);
}

/// Nobody listens: for tests of the expenses module on its own.
#[derive(Debug, Clone, Copy, Default)]
pub struct NobodyListens;

impl ExpenseDeletedListener for NobodyListens {
    fn expense_deleted(&self, _team: TeamId, _expense: ExpenseId) {}
}

/// [`NobodyListens`], ready to hand over.
#[cfg(test)]
pub(crate) fn nobody() -> std::sync::Arc<dyn ExpenseDeletedListener> {
    std::sync::Arc::new(NobodyListens)
}
