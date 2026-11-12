//! The black-box parity suite (ADR 0011). It talks only HTTP and SQL, so it depends on no other crate of the workspace.
//!
//! Java counterpart: the tests under `src/test/java/com/ledgerly/` that can be reproduced from outside the process.

#![forbid(unsafe_code)]
