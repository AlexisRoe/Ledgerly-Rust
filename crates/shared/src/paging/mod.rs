//! Paging helpers: the opaque cursor and keyset ("seek") paging for tables that only grow.
//!
//! Java counterpart: `shared/paging/` (`Cursor`, `KeysetPaging`). The audit trail uses them from M9; until then
//! they are tested against `expenses`. Offset paging for the expense list lives in the expenses module, as in
//! Java.

mod cursor;
mod keyset;

pub use cursor::{Cursor, InvalidCursor};
pub use keyset::{KeysetPage, fetch};
