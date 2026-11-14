//! Domain events and the port they are published through.
//!
//! Java counterpart: Spring's `ApplicationEventPublisher` with Spring Modulith's event publication registry.
//! Every event carries a v7 `eventId`, the instant it happened and ids only, never emails, amounts or names
//! (Java's ADR 0001).
//!
//! Until the outbox exists (M9.1) the app wires [`Discard`]: events are built and handed over at the point
//! where Java publishes them, inside the use case's transaction, and dropped. M9 replaces it with a publisher
//! that writes `event_publication` rows in that same transaction.

use std::fmt;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

/// An event any module can publish.
pub trait Event: fmt::Debug + Send + Sync {
    /// The event's own id, a UUIDv7.
    fn event_id(&self) -> Uuid;
    /// When it happened, by the application clock.
    fn occurred_at(&self) -> DateTime<Utc>;
    /// The name it is stored under, `ledgerly-rust:<module>.<Event>` (ADR 0005: never one of Java's names).
    fn event_type(&self) -> &'static str;
}

/// What a use case publishes through. `connection` is the use case's open transaction, so that an event is
/// stored if and only if the change that caused it commits.
pub trait Publisher: fmt::Debug + Send + Sync {
    /// Hands `event` over for delivery after the transaction commits.
    fn publish<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        event: &'a dyn Event,
    ) -> Pin<Box<dyn Future<Output = sqlx::Result<()>> + Send + 'a>>;
}

/// Drops every event: the publisher until the outbox exists (M9.1).
#[derive(Debug, Clone, Copy, Default)]
pub struct Discard;

impl Publisher for Discard {
    fn publish<'a>(
        &'a self,
        _connection: &'a mut PgConnection,
        _event: &'a dyn Event,
    ) -> Pin<Box<dyn Future<Output = sqlx::Result<()>> + Send + 'a>> {
        Box::pin(std::future::ready(Ok(())))
    }
}
