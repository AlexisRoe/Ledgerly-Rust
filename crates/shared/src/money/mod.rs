//! Money: an amount together with its currency.
//!
//! Java counterpart: `shared/money/` plus the JDK's `java.util.Currency`, whose data is reproduced in the
//! generated `currencies` table.

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

mod currencies;
mod currency;

pub use currency::{Currency, CurrencyError};
