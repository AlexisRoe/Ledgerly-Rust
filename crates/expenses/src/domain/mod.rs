//! The pure core of the expenses module: no I/O, no async, time only through `Clock` (CONCEPT §3, rule 4).

#![cfg_attr(not(test), deny(clippy::indexing_slicing, clippy::arithmetic_side_effects))]

mod action;
mod actor;
mod category;
mod error;
mod expense;
mod status;
mod text;

pub(crate) use category::Category;
pub(crate) use error::ExpenseError;
pub(crate) use expense::{Expense, NewExpense, Rejection, StoredExpense};
pub(crate) use status::ExpenseStatus;
pub(crate) use text::{Merchant, Note, RejectionReason};
// Only tests use these until the web layer serves the transitions (M3.4).
#[cfg(test)]
pub(crate) use {action::Action, actor::Actor, error::Forbidden, text::InvalidInput};
