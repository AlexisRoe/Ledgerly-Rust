//! The identity module: users, passwords, email verification, refresh tokens, JWTs and the JWKS endpoint.
//!
//! Java counterpart: `src/main/java/com/ledgerly/identity/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

#[cfg_attr(not(test), expect(dead_code, reason = "register and login use it from M5.3"))]
mod password;
