//! Domain events, the port they are published through, and the outbox that delivers them.
//!
//! Java counterpart: Spring's `ApplicationEventPublisher` with Spring Modulith's event publication registry.
//! Every event carries a v7 `eventId`, the instant it happened and ids only, never emails, amounts or names
//! (Java's ADR 0001).
//!
//! A use case publishes inside its own transaction; the [`Outbox`] writes one `event_publication` row per
//! interested [`Listener`] in that same transaction, so an event is stored if and only if the change that caused
//! it commits. After the commit the outbox delivers it, and keeps delivering until the listener succeeds. See
//! [`outbox`] for how.

mod outbox;

use std::fmt;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

pub use outbox::{AlreadySubscribed, Outbox};

/// Every `event_type` Rust writes starts with it; rows of any other type are Java's and Rust never reads,
/// delivers, completes or deletes them (ADR 0005).
pub const EVENT_TYPE_PREFIX: &str = "ledgerly-rust:";

/// An event any module can publish.
pub trait Event: fmt::Debug + Send + Sync {
    /// The event's own id, a UUIDv7. Stable across re-delivery, so a listener can recognise an event it has
    /// already handled.
    fn event_id(&self) -> Uuid;
    /// When it happened, by the application clock.
    fn occurred_at(&self) -> DateTime<Utc>;
    /// The name it is stored under, `ledgerly-rust:<module>.<Event>` (ADR 0005: never one of Java's names).
    fn event_type(&self) -> &'static str;
    /// The JSON stored in `event_publication.serialized_event`. The listener reads it back with the event's
    /// own `Deserialize`.
    ///
    /// # Errors
    ///
    /// The serialisation error; with the derived `Serialize` of ids, numbers and names it does not happen.
    fn payload(&self) -> serde_json::Result<String>;
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

/// Drops every event: for tests of one module that do not look at its events.
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

/// A delivery that did not succeed. The publication stays incomplete and is delivered again; the message is
/// logged, so it names kinds of failure, never values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DeliveryFailed(pub String);

/// What [`Listener::deliver`] returns.
pub type Delivery<'a> = Pin<Box<dyn Future<Output = Result<(), DeliveryFailed>> + Send + 'a>>;

/// Something that reacts to events after the publisher's transaction committed. Java: an
/// `@ApplicationModuleListener`, or a tracked `@TransactionalEventListener`.
///
/// Delivery is at least once: after a crash, or when the listener failed, the same event comes again. A
/// listener must therefore be idempotent (the audit trail ignores an event id it has, a scan skips a receipt
/// that is no longer pending, the import job leaves a finished import alone).
pub trait Listener: fmt::Debug + Send + Sync {
    /// Stored as `event_publication.listener_id`, so it must not change between releases while publications
    /// are open. `ledgerly-rust:<module>.<Listener>`.
    fn id(&self) -> &'static str;

    /// True for the event types it wants; one publication row is written for each.
    fn listens_to(&self, event_type: &str) -> bool;

    /// Handles one event, given as its type and [`Event::payload`]. `Ok` completes the publication.
    fn deliver<'a>(&'a self, event_type: &'a str, payload: &'a str) -> Delivery<'a>;
}

/// Reads a payload back into the event type a listener knows.
///
/// # Errors
///
/// [`DeliveryFailed`] naming the event type, if the JSON is not one of these. The payload itself is not
/// quoted.
pub fn read_payload<T: serde::de::DeserializeOwned>(event_type: &str, payload: &str) -> Result<T, DeliveryFailed> {
    serde_json::from_str(payload).map_err(|_| DeliveryFailed(format!("unreadable {event_type}")))
}
