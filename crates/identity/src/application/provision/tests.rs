//! Java: `src/test/java/com/ledgerly/identity/internal/keycloak/ExternalIdentityTest.java`, the provisioning
//! rules with hand-made identities (no provider needed: the token check is tested in `keycloak`, and against
//! a real Keycloak in the parity suite).

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use chrono::{TimeZone, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::events::{Event, Publisher};
use ledgerly_shared::id::UserId;
use uuid::Uuid;

use super::{Provision, ProvisioningError};

/// The event types the audit module records as `USER_REGISTERED` and `USER_VERIFIED`.
const REGISTERED: &str = "ledgerly-rust:identity.UserRegistered";
const VERIFIED: &str = "ledgerly-rust:identity.UserVerified";

/// Keeps the type of every event and how it prints, in order.
#[derive(Debug, Default)]
struct Recorded(Mutex<Vec<(&'static str, String)>>);

impl Publisher for Recorded {
    fn publish<'a>(
        &'a self,
        _connection: &'a mut sqlx::PgConnection,
        event: &'a dyn Event,
    ) -> Pin<Box<dyn Future<Output = sqlx::Result<()>> + Send + 'a>> {
        self.0.lock().unwrap().push((event.event_type(), format!("{event:?}")));
        Box::pin(std::future::ready(Ok(())))
    }
}

struct World {
    database: TestDatabase,
    provision: Provision,
    events: Arc<Recorded>,
}

impl World {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 17, 9, 0, 0).unwrap()));
        let events = Arc::new(Recorded::default());
        let publisher: Arc<dyn Publisher> = events.clone();
        let provision = Provision::new(database.pool().clone(), clock, publisher);
        Self {
            database,
            provision,
            events,
        }
    }

    async fn rows_for(&self, subject: &str) -> i64 {
        sqlx::query_scalar("select count(*) from users where external_subject = $1")
            .bind(subject)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn verified(&self, subject: &str) -> bool {
        sqlx::query_scalar("select verified from users where external_subject = $1")
            .bind(subject)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn insert_local_user(&self, id: Uuid, email: &str, verified: bool) {
        sqlx::query(
            "insert into users (id, email, password_hash, verified, created_at) values ($1, $2, 'hash', $3, now())",
        )
        .bind(id)
        .bind(email)
        .bind(verified)
        .execute(self.database.pool())
        .await
        .unwrap();
    }

    fn event_types(&self) -> Vec<&'static str> {
        self.events.0.lock().unwrap().iter().map(|(kind, _)| *kind).collect()
    }
}

fn new_subject() -> String {
    Uuid::new_v4().to_string()
}

fn new_email() -> String {
    format!("kc-{}@example.com", Uuid::new_v4())
}

#[tokio::test]
async fn the_first_token_creates_a_user_with_our_own_id_and_no_password() {
    let world = World::new().await;
    let (subject, email) = (new_subject(), new_email());

    let id = world
        .provision
        .resolve(&subject, Some(&email.to_uppercase()), true)
        .await
        .unwrap();

    let row: (Uuid, String, bool, Option<String>) =
        sqlx::query_as("select id, email, verified, password_hash from users where external_subject = $1")
            .bind(&subject)
            .fetch_one(world.database.pool())
            .await
            .unwrap();
    assert_eq!(row.0, id.as_uuid());
    assert_ne!(id.to_string(), subject, "our id, not the provider's");
    assert_eq!(row.1, email, "normalised");
    assert!(row.2);
    assert_eq!(row.3, None);
}

#[tokio::test]
async fn the_same_subject_is_the_same_user_every_time() {
    let world = World::new().await;
    let (subject, email) = (new_subject(), new_email());

    let first = world.provision.resolve(&subject, Some(&email), true).await.unwrap();
    let second = world.provision.resolve(&subject, Some(&email), true).await.unwrap();
    // The subject wins over a changed email.
    let third = world
        .provision
        .resolve(&subject, Some(&format!("changed-{email}")), true)
        .await
        .unwrap();

    assert_eq!(second, first);
    assert_eq!(third, first);
    assert_eq!(world.rows_for(&subject).await, 1);
}

/// Java checks the audit trail; here the events the audit module turns into rows.
#[tokio::test]
async fn creating_a_user_is_published_without_any_email() {
    let world = World::new().await;

    let id = world
        .provision
        .resolve(&new_subject(), Some(&new_email()), true)
        .await
        .unwrap();

    assert_eq!(world.event_types(), [REGISTERED, VERIFIED]);
    for (_, printed) in world.events.0.lock().unwrap().iter() {
        assert!(printed.contains(&id.to_string()), "{printed}");
        assert!(!printed.contains('@'), "{printed}");
    }
}

#[tokio::test]
async fn an_unverified_email_stays_unverified_until_the_provider_says_otherwise() {
    let world = World::new().await;
    let (subject, email) = (new_subject(), new_email());

    world.provision.resolve(&subject, Some(&email), false).await.unwrap();
    assert!(!world.verified(&subject).await);
    assert_eq!(world.event_types(), [REGISTERED]);

    world.provision.resolve(&subject, Some(&email), true).await.unwrap();
    assert!(world.verified(&subject).await);

    // A later token never takes it back.
    world.provision.resolve(&subject, Some(&email), false).await.unwrap();
    assert!(world.verified(&subject).await);
    assert_eq!(world.event_types(), [REGISTERED, VERIFIED]);
}

#[tokio::test]
async fn a_new_person_without_an_email_claim_is_refused() {
    let world = World::new().await;
    let subject = new_subject();

    for email in [None, Some("  ")] {
        assert!(matches!(
            world.provision.resolve(&subject, email, true).await,
            Err(ProvisioningError::Refused)
        ));
    }
    assert_eq!(world.rows_for(&subject).await, 0);
}

#[tokio::test]
async fn an_existing_local_account_is_linked_when_the_provider_verifies_the_same_email() {
    let world = World::new().await;
    let (existing, email, subject) = (Uuid::new_v4(), new_email(), new_subject());
    world.insert_local_user(existing, &email, false).await;

    let id = world
        .provision
        .resolve(&subject, Some(&email.to_uppercase()), true)
        .await
        .unwrap();

    assert_eq!(id, UserId::from_uuid(existing), "same account, no duplicate");
    assert_eq!(world.rows_for(&subject).await, 1);
    let accounts: i64 = sqlx::query_scalar("select count(*) from users where email = $1")
        .bind(&email)
        .fetch_one(world.database.pool())
        .await
        .unwrap();
    assert_eq!(accounts, 1);
    assert!(world.verified(&subject).await);
    assert_eq!(world.event_types(), [VERIFIED]);
}

#[tokio::test]
async fn an_existing_local_account_is_never_linked_on_an_unverified_email() {
    let world = World::new().await;
    let (existing, email, subject) = (Uuid::new_v4(), new_email(), new_subject());
    world.insert_local_user(existing, &email, true).await;

    let refused = world.provision.resolve(&subject, Some(&email), false).await;

    assert!(matches!(refused, Err(ProvisioningError::Refused)));
    assert_eq!(world.rows_for(&subject).await, 0);
    let linked: Option<String> = sqlx::query_scalar("select external_subject from users where id = $1")
        .bind(existing)
        .fetch_one(world.database.pool())
        .await
        .unwrap();
    assert_eq!(linked, None);
}

#[tokio::test]
async fn an_account_already_linked_to_someone_else_cannot_be_taken_over() {
    let world = World::new().await;
    let email = new_email();
    world
        .provision
        .resolve(&new_subject(), Some(&email), true)
        .await
        .unwrap();

    let refused = world.provision.resolve(&new_subject(), Some(&email), true).await;

    assert!(matches!(refused, Err(ProvisioningError::Refused)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn simultaneous_first_requests_of_the_same_person_create_one_user() {
    let world = World::new().await;
    let (subject, email) = (new_subject(), new_email());
    let gate = Arc::new(tokio::sync::Barrier::new(8));

    let attempts: Vec<_> = (0..8)
        .map(|_| {
            let (provision, gate) = (world.provision.clone(), gate.clone());
            let (subject, email) = (subject.clone(), email.clone());
            tokio::spawn(async move {
                gate.wait().await;
                provision.resolve(&subject, Some(&email), true).await
            })
        })
        .collect();
    let mut ids = Vec::new();
    for attempt in attempts {
        ids.push(attempt.await.unwrap().unwrap());
    }

    ids.dedup();
    assert_eq!(ids.len(), 1, "{ids:?}");
    assert_eq!(world.rows_for(&subject).await, 1);
    assert_eq!(world.event_types(), [REGISTERED, VERIFIED]);
}
