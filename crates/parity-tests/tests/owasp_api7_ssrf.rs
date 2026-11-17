//! OWASP API7:2023 Server Side Request Forgery: no feature fetches a URL a client supplies, and only
//! infrastructure code may open outbound connections.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api7ServerSideRequestForgeryTest.java`.
//!
//! This file has no tests on purpose. Java proves the rule with ArchUnit on its own classes, which cannot be
//! seen from outside the process. All three of its tests are listed in `NOT_PARITY.md`; the Rust proof is the
//! dependency check in `crates/app/tests/architecture.rs` (M10).
