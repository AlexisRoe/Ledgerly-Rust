//! The durable outbox in `event_publication` (V7, Spring Modulith's schema). Java counterpart: Modulith's event
//! publication registry with `republish-outstanding-events-on-restart: true`, and `shared/config/EventMaintenance`.
//!
//! - **Publishing** writes one row per interested listener in the publisher's transaction (`status`
//!   `PUBLISHED`), and a `pg_notify` on [`CHANNEL`] with the row's id. Postgres sends a notification only when
//!   its transaction commits, so a rolled-back change is never delivered.
//! - **Delivering:** the dispatcher listens on the channel and hands each committed row to its listener, in a
//!   task of its own (Java's `@Async`). Success sets `completion_date` (`COMPLETED`); a failure leaves it open
//!   (`FAILED`).
//! - **Re-delivery:** at start-up every open row of Rust's is delivered again, and every minute (first after a
//!   minute) those published more than two minutes ago (`RESUBMITTED`, `completion_attempts` + 1,
//!   `last_resubmission_date`). Two minutes, as in Java, so that a listener that is merely slow is not started
//!   twice; within this process a row being delivered is never handed out a second time anyway.
//! - **Purging:** completed rows older than seven days are deleted every night at 03:17 UTC (Java's cron
//!   `0 17 3 * * *` in the server's zone, which is UTC in the container).
//!
//! Only rows whose `event_type` starts with [`EVENT_TYPE_PREFIX`] are ever read, changed or deleted (ADR 0005).
//! A Java row left in the table is Java's to finish.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use chrono::{DateTime, NaiveTime, TimeDelta, Utc};
use sqlx::postgres::PgListener;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::{EVENT_TYPE_PREFIX, Event, Listener, Publisher};
use crate::clock::Clock;

/// The notification channel. Only ids travel on it.
pub(crate) const CHANNEL: &str = "ledgerly_rust_event_publication";
/// `EventMaintenance.MINIMUM_AGE`.
const STUCK_AFTER: TimeDelta = TimeDelta::minutes(2);
/// `ledgerly.events.resubmit-interval` (`PT1M`), also the initial delay.
const RESUBMIT_EVERY: Duration = Duration::from_secs(60);
/// `EventMaintenance.KEEP_COMPLETED`.
const KEEP_COMPLETED: TimeDelta = TimeDelta::days(7);
/// How long to wait before listening again after the connection failed.
const RECONNECT_AFTER: Duration = Duration::from_secs(5);

/// The listeners were already set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the outbox's listeners are already set")]
pub struct AlreadySubscribed;

/// The outbox: a [`Publisher`] and, once [`Outbox::start`]ed, the dispatcher that delivers what was
/// published. Cheap to clone; clones share everything.
#[derive(Debug, Clone)]
pub struct Outbox {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    listeners: OnceLock<Vec<Arc<dyn Listener>>>,
    /// The ids being delivered by this process right now.
    in_flight: Mutex<HashSet<Uuid>>,
}

/// Removes an id from the in-flight set when the delivery ends, also when the listener panicked.
struct InFlight {
    inner: Arc<Inner>,
    id: Uuid,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.inner.in_flight().remove(&self.id);
    }
}

impl Inner {
    fn in_flight(&self) -> std::sync::MutexGuard<'_, HashSet<Uuid>> {
        // Nothing panics while holding the lock; if something did, the set is still a set.
        self.in_flight.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Outbox {
    /// An outbox without listeners; set them with [`Outbox::subscribe`] before anything is published.
    #[must_use]
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Inner {
                pool,
                clock,
                listeners: OnceLock::new(),
                in_flight: Mutex::new(HashSet::new()),
            }),
        }
    }

    /// Sets the listeners, once. They are known only after the modules are built, and the modules need the
    /// publisher, hence the two steps.
    ///
    /// # Errors
    ///
    /// [`AlreadySubscribed`] on a second call.
    pub fn subscribe(&self, listeners: Vec<Arc<dyn Listener>>) -> Result<(), AlreadySubscribed> {
        self.inner.listeners.set(listeners).map_err(|_| AlreadySubscribed)
    }

    fn listeners(&self) -> &[Arc<dyn Listener>] {
        self.inner.listeners.get().map_or(&[], Vec::as_slice)
    }

    /// Starts the dispatcher in a task of its own: it listens for new publications, delivers every open one
    /// once, then re-delivers stuck ones every minute and purges old completed ones every night. Aborting the
    /// returned task stops it; deliveries already running finish or are abandoned with the runtime, and an
    /// abandoned one is delivered again at the next start.
    #[must_use]
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let outbox = self.clone();
        tokio::spawn(async move { outbox.run().await })
    }

    async fn run(self) {
        // Listening first, so that nothing committed between the catch-up and the first notification is missed.
        let mut notifications = self.listen().await;
        if notifications.is_none() {
            tracing::warn!("Cannot listen for events yet; stuck ones are re-delivered every minute");
        }
        self.redeliver(None).await;
        let mut resubmit = tokio::time::interval_at(tokio::time::Instant::now() + RESUBMIT_EVERY, RESUBMIT_EVERY);
        resubmit.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let purge = tokio::time::sleep(until_next_purge(self.inner.clock.now()));
        tokio::pin!(purge);
        loop {
            tokio::select! {
                wake = wait(&mut notifications) => match wake {
                    Wake::Published(id) => self.dispatch(id),
                    // The connection was lost and is back: what was published meanwhile is delivered now.
                    Wake::Reconnected => {
                        self.redeliver(None).await;
                    }
                    Wake::Lost(error) => {
                        tracing::warn!("Stopped listening for events ({error}); trying again");
                        notifications = None;
                    }
                    Wake::Retry => {
                        notifications = self.listen().await;
                        if notifications.is_some() {
                            self.redeliver(None).await;
                        }
                    }
                },
                _ = resubmit.tick() => {
                    self.redeliver(Some(STUCK_AFTER)).await;
                }
                () = &mut purge => {
                    match self.purge_completed(KEEP_COMPLETED).await {
                        Ok(_) => tracing::info!("Deleted completed event publications older than PT168H"),
                        Err(error) => tracing::warn!("Could not delete completed event publications: {error}"),
                    }
                    purge.as_mut().reset(tokio::time::Instant::now() + until_next_purge(self.inner.clock.now()));
                }
            }
        }
    }

    async fn listen(&self) -> Option<PgListener> {
        let mut listener = PgListener::connect_with(&self.inner.pool).await.ok()?;
        listener.listen(CHANNEL).await.ok()?;
        Some(listener)
    }

    /// Delivers again every open publication of Rust's that is not being delivered right now; with `older_than`
    /// only those published longer ago than that. Returns how many were handed out.
    pub async fn redeliver(&self, older_than: Option<TimeDelta>) -> usize {
        let now = self.inner.clock.now();
        let busy: Vec<Uuid> = self.inner.in_flight().iter().copied().collect();
        let ids = sqlx::query_scalar!(
            r#"update event_publication
                  set status = 'RESUBMITTED',
                      completion_attempts = coalesce(completion_attempts, 0) + 1,
                      last_resubmission_date = $1
                where completion_date is null
                  and starts_with(event_type, $2)
                  and ($3::timestamptz is null or publication_date < $3)
                  and id <> all($4)
            returning id"#,
            now,
            EVENT_TYPE_PREFIX,
            older_than.map(|age| now - age),
            &busy,
        )
        .fetch_all(&self.inner.pool)
        .await;
        match ids {
            Ok(ids) => {
                let count = ids.len();
                for id in ids {
                    self.dispatch(id);
                }
                count
            }
            Err(error) => {
                tracing::warn!("Could not look for undelivered events: {error}");
                0
            }
        }
    }

    /// Deletes Rust's completed publications older than `age`. Returns how many.
    ///
    /// # Errors
    ///
    /// The database error.
    pub async fn purge_completed(&self, age: TimeDelta) -> sqlx::Result<u64> {
        let deleted = sqlx::query!(
            "delete from event_publication where completion_date < $1 and starts_with(event_type, $2)",
            self.inner.clock.now() - age,
            EVENT_TYPE_PREFIX,
        )
        .execute(&self.inner.pool)
        .await?;
        Ok(deleted.rows_affected())
    }

    /// Delivers one publication in a task of its own, unless this process is delivering it already.
    fn dispatch(&self, id: Uuid) {
        if !self.inner.in_flight().insert(id) {
            return;
        }
        let guard = InFlight {
            inner: self.inner.clone(),
            id,
        };
        let outbox = self.clone();
        tokio::spawn(async move {
            outbox.deliver(id).await;
            drop(guard);
        });
    }

    async fn deliver(&self, id: Uuid) {
        let row = sqlx::query!(
            "select listener_id, event_type, serialized_event from event_publication
              where id = $1 and completion_date is null and starts_with(event_type, $2)",
            id,
            EVENT_TYPE_PREFIX,
        )
        .fetch_optional(&self.inner.pool)
        .await;
        let row = match row {
            // Already completed (by an earlier delivery), or not Rust's.
            Ok(None) => return,
            Ok(Some(row)) => row,
            Err(error) => {
                tracing::warn!("Could not read event publication {id}: {error}");
                return;
            }
        };
        let Some(listener) = self
            .listeners()
            .iter()
            .find(|listener| listener.id() == row.listener_id)
        else {
            tracing::warn!("No listener {} for event publication {id}", row.listener_id);
            return;
        };
        let outcome = listener.deliver(&row.event_type, &row.serialized_event).await;
        let marked = match &outcome {
            Ok(()) => sqlx::query!(
                "update event_publication set completion_date = $2, status = 'COMPLETED' where id = $1",
                id,
                self.inner.clock.now(),
            )
            .execute(&self.inner.pool)
            .await
            .map(|_| ()),
            Err(failure) => {
                tracing::warn!("Delivering {} to {} failed: {failure}", row.event_type, row.listener_id);
                sqlx::query!("update event_publication set status = 'FAILED' where id = $1", id)
                    .execute(&self.inner.pool)
                    .await
                    .map(|_| ())
            }
        };
        if let Err(error) = marked {
            tracing::warn!("Could not record the delivery of event publication {id}: {error}");
        }
    }
}

impl Publisher for Outbox {
    fn publish<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        event: &'a dyn Event,
    ) -> std::pin::Pin<Box<dyn Future<Output = sqlx::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let event_type = event.event_type();
            if !event_type.starts_with(EVENT_TYPE_PREFIX) {
                return Err(sqlx::Error::Configuration(
                    format!("event type {event_type} is not one of Rust's").into(),
                ));
            }
            let listeners: Vec<&Arc<dyn Listener>> = self
                .listeners()
                .iter()
                .filter(|listener| listener.listens_to(event_type))
                .collect();
            if listeners.is_empty() {
                return Ok(());
            }
            let payload = event.payload().map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
            let now = self.inner.clock.now();
            for listener in listeners {
                // Modulith's ids are random (v4).
                let id = Uuid::new_v4();
                sqlx::query!(
                    "insert into event_publication
                         (id, listener_id, event_type, serialized_event, publication_date, status, completion_attempts)
                     values ($1, $2, $3, $4, $5, 'PUBLISHED', 0)",
                    id,
                    listener.id(),
                    event_type,
                    payload,
                    now,
                )
                .execute(&mut *connection)
                .await?;
                sqlx::query!("select pg_notify($1, $2)", CHANNEL, id.to_string())
                    .execute(&mut *connection)
                    .await?;
            }
            Ok(())
        })
    }
}

/// Why the dispatcher woke up.
enum Wake {
    /// A publication committed.
    Published(Uuid),
    /// The connection was lost and re-established; notifications may have been missed.
    Reconnected,
    /// The connection is gone.
    Lost(sqlx::Error),
    /// Not listening, and it is time to try again.
    Retry,
}

async fn wait(notifications: &mut Option<PgListener>) -> Wake {
    let Some(listener) = notifications.as_mut() else {
        tokio::time::sleep(RECONNECT_AFTER).await;
        return Wake::Retry;
    };
    loop {
        match listener.try_recv().await {
            // Nobody else sends on the channel; anything that is not an id is skipped.
            Ok(Some(notification)) => {
                if let Ok(id) = notification.payload().parse::<Uuid>() {
                    return Wake::Published(id);
                }
            }
            Ok(None) => return Wake::Reconnected,
            Err(error) => return Wake::Lost(error),
        }
    }
}

/// From `now` to the next 03:17:00 UTC.
fn until_next_purge(now: DateTime<Utc>) -> Duration {
    let at = NaiveTime::from_hms_opt(3, 17, 0).unwrap_or(NaiveTime::MIN);
    let today = now.date_naive().and_time(at).and_utc();
    let next = if today > now { today } else { today + TimeDelta::days(1) };
    (next - now).to_std().unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    #[test]
    fn the_purge_runs_at_the_next_3_17_utc() {
        let table = [
            ("2026-11-16T01:00:00Z", 2 * 3600 + 17 * 60),
            ("2026-11-16T03:16:59Z", 1),
            ("2026-11-16T03:17:00Z", 24 * 3600),
            ("2026-11-16T22:00:00Z", 5 * 3600 + 17 * 60),
        ];
        for (now, seconds) in table {
            assert_eq!(until_next_purge(instant(now)), Duration::from_secs(seconds), "{now}");
        }
    }
}
