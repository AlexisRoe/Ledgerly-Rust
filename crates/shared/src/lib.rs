//! Building blocks every Ledgerly module uses: typed ids, `Money`, roles, the clock and the configuration.
//!
//! Java counterpart: the open `shared` module
//! (`src/main/java/com/ledgerly/shared/` in Ledgerly-Java at the commit in `vendor/ledgerly-java/SOURCE`).

#![forbid(unsafe_code)]

pub mod clock;
pub mod config;
pub mod id;
pub mod money;
pub mod role;
pub mod text;
