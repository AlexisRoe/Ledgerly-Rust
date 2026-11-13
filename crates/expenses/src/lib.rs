//! The expenses module: the `Expense` aggregate and its approval workflow.
//!
//! Java counterpart: `src/main/java/com/ledgerly/expenses/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod domain;
// Used by the application services from M2.4 on; `update` and `delete` only once the web layer arrives (M3).
// `expect` instead of `allow`, so that this attribute stops compiling, and has to go, once that is no longer true.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "update and delete have no caller outside the tests until M3")
)]
mod persistence;
mod receipt_checks;

pub use receipt_checks::{ReceiptChecks, ReceiptSummary};
