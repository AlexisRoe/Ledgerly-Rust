//! The pure core of the expenses module: no I/O, no async, time only through `Clock` (CONCEPT §3, rule 4).

#![cfg_attr(not(test), deny(clippy::indexing_slicing, clippy::arithmetic_side_effects))]

mod action;
mod actor;
mod category;
mod error;
mod expense;
mod status;
mod text;

pub use category::Category;
pub(crate) use error::ExpenseError;
pub use expense::NewExpense;
pub(crate) use expense::{Expense, Rejection, StoredExpense, check_fields};
pub(crate) use status::ExpenseStatus;
pub use text::InvalidInput;
pub(crate) use text::{Merchant, Note, RejectionReason};
pub(crate) use {action::Action, actor::Actor};
// Only tests name these; production code meets them inside `ExpenseError`.
#[cfg(test)]
pub(crate) use error::Forbidden;
