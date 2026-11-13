//! The black-box parity suite (ADR 0011). It talks only HTTP and SQL, so it depends on no other crate of the
//! workspace and can be pointed at Ledgerly-Rust or at Ledgerly-Java.
//!
//! Java counterpart: the tests under `src/test/java/com/ledgerly/` that can be reproduced from outside the
//! process (`docs/reference/java-test-inventory.md`).
//!
//! What a test gets from here:
//!
//! - [`Target`]: where the system under test is (`PARITY_BASE_URL`, `PARITY_DATABASE_URL`, …).
//! - [`http::Client`]: requests against the API or the management port. Every response is checked against the
//!   vendored `api/openapi.yaml` unless the test opts out for a path the spec does not describe.
//! - [`fixtures::Fixtures`]: starting state written with SQL into the shared schema, as Java's tests do with
//!   their seeders. Every helper uses fresh ids, so tests do not see each other.
//!
//! The tests live in `tests/`, each in a module named after the milestone that makes it pass (`m5`, `m6`, …),
//! and carry `#[ignore = "M<N>"]` until then. `just parity` starts the app once per run group and prints the
//! count of passed, ignored and failed tests per milestone.

#![forbid(unsafe_code)]
// This crate is test code from top to bottom: a fixture that cannot be written or a response that cannot be
// read is a failed test, and a panic with a message is how a test fails.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod encoding;
pub mod fixtures;
pub mod groups;
pub mod http;
pub mod report;
pub mod spec;
mod target;

pub use target::Target;
