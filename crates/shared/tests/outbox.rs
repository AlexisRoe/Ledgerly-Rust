//! The outbox against a real database: an event is delivered only once its transaction commits, exactly once
//! even when the dispatcher dies before delivering, and Java's rows are never touched.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::events::{Delivery, DeliveryFailed, Event, Listener, Outbox, Publisher, read_payload};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Pinged {
    event_id: Uuid,
    occurred_at: DateTime<Utc>,
}

impl Event for Pinged {
    fn event_id(&self) -> Uuid {
        self.event_id
    }
    fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }
    fn event_type(&self) -> &'static str {
        "ledgerly-rust:tests.Pinged"
    }
    fn payload(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

/// Counts what it received. While `stuck` is set, a delivery never ends: a dispatcher that dies mid-way.
#[derive(Debug, Default)]
struct Counting {
    delivered: AtomicUsize,
    failures_left: AtomicUsize,
    stuck: std::sync::atomic::AtomicBool,
    started: Notify,
}

#[derive(Debug, Clone)]
struct CountingListener(Arc<Counting>);

impl Listener for CountingListener {
    fn id(&self) -> &'static str {
        "ledgerly-rust:tests.Counting"
    }
    fn listens_to(&self, event_type: &str) -> bool {
        event_type == "ledgerly-rust:tests.Pinged"
    }
    fn deliver<'a>(&'a self, event_type: &'a str, payload: &'a str) -> Delivery<'a> {
        Box::pin(async move {
            let _event: Pinged = read_payload(event_type, payload)?;
            self.0.started.notify_one();
            if self.0.stuck.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            if self
                .0
                .failures_left
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                return Err(DeliveryFailed("the scanner is down".to_owned()));
            }
            self.0.delivered.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

fn clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::at("2026-11-16T10:00:00Z".parse::<DateTime<Utc>>().unwrap()))
}

fn outbox(pool: &PgPool, clock: &Arc<FixedClock>, counting: &Arc<Counting>) -> Outbox {
    let outbox = Outbox::new(pool.clone(), clock.clone() as Arc<dyn Clock>);
    outbox
        .subscribe(vec![Arc::new(CountingListener(counting.clone()))])
        .unwrap();
    outbox
}

async fn publish(pool: &PgPool, outbox: &Outbox, clock: &Arc<FixedClock>, commit: bool) {
    let mut transaction = pool.begin().await.unwrap();
    let event = Pinged {
        event_id: Uuid::now_v7(),
        occurred_at: clock.now(),
    };
    outbox.publish(&mut transaction, &event).await.unwrap();
    if commit {
        transaction.commit().await.unwrap();
    } else {
        transaction.rollback().await.unwrap();
    }
}

async fn open_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("select count(*) from event_publication where completion_date is null")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn until(what: &str, mut done: impl AsyncFnMut() -> bool) {
    for _ in 0..200 {
        if done().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn an_event_is_delivered_after_its_transaction_commits() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let (clock, counting) = (clock(), Arc::new(Counting::default()));
    let outbox = outbox(pool, &clock, &counting);
    let dispatcher = outbox.start();
    // The dispatcher listens before it catches up; give it the moment to do both.
    tokio::time::sleep(Duration::from_millis(300)).await;

    publish(pool, &outbox, &clock, true).await;

    until("the delivery", async || counting.delivered.load(Ordering::SeqCst) == 1).await;
    until("the completion", async || open_rows(pool).await == 0).await;
    let (listener, status): (String, String) =
        sqlx::query_as("select listener_id, status from event_publication")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(listener, "ledgerly-rust:tests.Counting");
    assert_eq!(status, "COMPLETED");
    dispatcher.abort();
}

#[tokio::test]
async fn a_rolled_back_event_is_neither_stored_nor_delivered() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let (clock, counting) = (clock(), Arc::new(Counting::default()));
    let outbox = outbox(pool, &clock, &counting);
    let dispatcher = outbox.start();
    tokio::time::sleep(Duration::from_millis(300)).await;

    publish(pool, &outbox, &clock, false).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(counting.delivered.load(Ordering::SeqCst), 0);
    let rows: i64 = sqlx::query_scalar("select count(*) from event_publication")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    dispatcher.abort();
}

/// The plan's acceptance test for M9.1.
#[tokio::test]
async fn a_dispatcher_killed_between_commit_and_delivery_delivers_exactly_once_after_a_restart() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let (clock, counting) = (clock(), Arc::new(Counting::default()));
    counting.stuck.store(true, Ordering::SeqCst);
    let first = outbox(pool, &clock, &counting);
    let dispatcher = first.start();
    tokio::time::sleep(Duration::from_millis(300)).await;
    publish(pool, &first, &clock, true).await;
    // The delivery has begun and will never finish: the process dies now.
    tokio::time::timeout(Duration::from_secs(5), counting.started.notified())
        .await
        .unwrap();
    dispatcher.abort();
    drop(first);
    assert_eq!(open_rows(pool).await, 1);

    counting.stuck.store(false, Ordering::SeqCst);
    let second = outbox(pool, &clock, &counting);
    let restarted = second.start();

    until("the delivery after the restart", async || open_rows(pool).await == 0).await;
    // Once completed, nothing delivers it again: not the next re-delivery, not another restart.
    clock.advance(TimeDelta::minutes(5));
    assert_eq!(second.redeliver(Some(TimeDelta::minutes(2))).await, 0);
    restarted.abort();
    let third = outbox(pool, &clock, &counting).start();
    tokio::time::sleep(Duration::from_millis(300)).await;
    third.abort();
    assert_eq!(counting.delivered.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_failed_delivery_stays_open_and_is_delivered_again_once_stuck() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let (clock, counting) = (clock(), Arc::new(Counting::default()));
    counting.failures_left.store(1, Ordering::SeqCst);
    let outbox = outbox(pool, &clock, &counting);
    let dispatcher = outbox.start();
    tokio::time::sleep(Duration::from_millis(300)).await;
    publish(pool, &outbox, &clock, true).await;
    until("the failed attempt", async || {
        sqlx::query_scalar::<_, String>("select status from event_publication")
            .fetch_one(pool)
            .await
            .unwrap()
            == "FAILED"
    })
    .await;

    // Not yet two minutes old: left alone.
    clock.advance(TimeDelta::seconds(119));
    assert_eq!(outbox.redeliver(Some(TimeDelta::minutes(2))).await, 0);
    clock.advance(TimeDelta::seconds(2));
    assert_eq!(outbox.redeliver(Some(TimeDelta::minutes(2))).await, 1);

    until("the second attempt", async || open_rows(pool).await == 0).await;
    let attempts: Option<i32> = sqlx::query_scalar("select completion_attempts from event_publication")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(attempts, Some(1));
    assert_eq!(counting.delivered.load(Ordering::SeqCst), 1);
    dispatcher.abort();
}

#[tokio::test]
async fn javas_rows_are_never_delivered_completed_or_purged() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let (clock, counting) = (clock(), Arc::new(Counting::default()));
    let long_ago = clock.now() - TimeDelta::days(30);
    for (id, completed) in [(Uuid::new_v4(), None), (Uuid::new_v4(), Some(long_ago))] {
        sqlx::query(
            "insert into event_publication (id, listener_id, event_type, serialized_event, publication_date, \
             completion_date) values ($1, 'ledgerly-rust:tests.Counting', \
             'com.ledgerly.receipts.events.ReceiptEvent$Uploaded', '{}', $2, $3)",
        )
        .bind(id)
        .bind(long_ago)
        .bind(completed)
        .execute(pool)
        .await
        .unwrap();
    }
    let outbox = outbox(pool, &clock, &counting);

    assert_eq!(outbox.redeliver(None).await, 0);
    assert_eq!(outbox.purge_completed(TimeDelta::days(7)).await.unwrap(), 0);
    let rows: i64 = sqlx::query_scalar("select count(*) from event_publication where status is null")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(rows, 2);
}

#[tokio::test]
async fn completed_rows_are_purged_after_seven_days() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let (clock, counting) = (clock(), Arc::new(Counting::default()));
    let outbox = outbox(pool, &clock, &counting);
    for age in [TimeDelta::days(8), TimeDelta::days(6)] {
        sqlx::query(
            "insert into event_publication (id, listener_id, event_type, serialized_event, publication_date, \
             completion_date, status) values ($1, 'ledgerly-rust:tests.Counting', 'ledgerly-rust:tests.Pinged', \
             '{}', $2, $2, 'COMPLETED')",
        )
        .bind(Uuid::new_v4())
        .bind(clock.now() - age)
        .execute(pool)
        .await
        .unwrap();
    }

    assert_eq!(outbox.purge_completed(TimeDelta::days(7)).await.unwrap(), 1);
}
