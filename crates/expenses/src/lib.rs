//! The expenses module: the `Expense` aggregate and its approval workflow.
//!
//! Java counterpart: `src/main/java/com/ledgerly/expenses/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

// Nothing outside the domain uses it until the application layer arrives in M2. `expect` instead of `allow`, so
// that this attribute stops compiling, and has to go, as soon as that is no longer true.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the domain is only exercised by its unit tests until M2")
)]
mod domain;
