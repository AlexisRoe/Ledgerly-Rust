//! A link shortener: `POST /links` stores a target URL under a short slug, `GET /{slug}` redirects to it, and
//! `GET /links/{slug}` shows the link with its visit count.
//!
//! Layers, inside out, the way Ledgerly's crates are built:
//! - [`domain`]: the types, which can only hold valid values;
//! - [`store`]: the port ([`store::LinkStore`]) and an in-memory adapter;
//! - [`web`]: axum handlers, and errors turned into responses;
//! - [`visits`]: a background task that counts visits in batches.

pub mod domain;
pub mod store;
pub mod visits;
pub mod web;
