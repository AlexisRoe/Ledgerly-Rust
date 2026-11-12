//! Wires the modules into the `ledgerly` binary: configuration, router, layers, seeding and shutdown.
//!
//! Java counterpart: `LedgerlyApplication` and the Spring context, in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

pub mod config;
