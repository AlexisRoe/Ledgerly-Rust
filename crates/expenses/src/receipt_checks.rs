//! What expenses needs to know about receipts when an expense is submitted.
//!
//! Expenses owns this interface and the receipts module implements it (M7), so every dependency points from
//! receipts to expenses and the two crates form no cycle (Java ADR 0002).
//!
//! Java counterpart: `expenses/ReceiptChecks.java` and `expenses/ReceiptSummary.java`.

use std::fmt;

use ledgerly_shared::id::{ExpenseId, TeamId};

/// Receipt counts for one expense, by scan status.
///
/// The counts are unsigned, so the negative counts Java's record has to reject cannot be written down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiptSummary {
    accepted: u32,
    pending_scan: u32,
}

impl ReceiptSummary {
    #[must_use]
    pub const fn new(accepted: u32, pending_scan: u32) -> Self {
        Self { accepted, pending_scan }
    }

    /// Receipts that passed the malware scan.
    #[must_use]
    pub const fn accepted(&self) -> u32 {
        self.accepted
    }

    /// Receipts whose scan has not finished.
    #[must_use]
    pub const fn pending_scan(&self) -> u32 {
        self.pending_scan
    }
}

/// Implemented by the receipts module, called by expenses before a submit.
///
/// Unlike Java's `summaryFor(ExpenseId)`, the team is part of the question: no read of team data goes without a
/// team id (CLAUDE.md, OWASP API1).
///
/// Synchronous and infallible for now, because M0 has no I/O. It becomes `async` and fallible when a database
/// stands behind it.
pub trait ReceiptChecks: fmt::Debug + Send + Sync {
    /// Counts the receipts attached to the expense. Submitting needs at least one accepted and none pending.
    fn summary_for(&self, team_id: TeamId, expense_id: ExpenseId) -> ReceiptSummary;
}

/// Until the receipts module exists (M7), no expense has a receipt.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoReceiptsYet;

impl ReceiptChecks for NoReceiptsYet {
    fn summary_for(&self, _team_id: TeamId, _expense_id: ExpenseId) -> ReceiptSummary {
        ReceiptSummary::new(0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_carries_the_two_counts() {
        let summary = ReceiptSummary::new(2, 1);
        assert_eq!(summary.accepted(), 2);
        assert_eq!(summary.pending_scan(), 1);
    }

    #[test]
    fn the_receipts_module_can_implement_the_trait_and_be_used_as_a_trait_object() {
        #[derive(Debug)]
        struct TwoAccepted;

        impl ReceiptChecks for TwoAccepted {
            fn summary_for(&self, _team_id: TeamId, _expense_id: ExpenseId) -> ReceiptSummary {
                ReceiptSummary::new(2, 0)
            }
        }

        let checks: Box<dyn ReceiptChecks> = Box::new(TwoAccepted);
        assert_eq!(
            checks.summary_for(TeamId::new_v7(), ExpenseId::new_v7()),
            ReceiptSummary::new(2, 0)
        );
    }
}
