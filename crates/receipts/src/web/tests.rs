//! The receipts endpoints in-process (axum `oneshot`) against real Postgres, with the objects in memory and
//! roles from the real teams lookup.
//!
//! Java: `ReceiptsApiTest`, which also uses an object store of its own (`FileSystemObjectStorage`). The
//! black-box versions, against SeaweedFS and ClamAV, are in the parity suite; these add what only shows from
//! inside: what is stored, which events are published, and what is never read.

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body as HttpBody;
use axum::http::{Method, Request, StatusCode};
use chrono::{TimeZone, Utc};
use ledgerly_expenses::ExpensesFacade;
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::config::Limits;
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::events::{Event, Listener, Outbox, Publisher};
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::storage::ObjectStorage;
use ledgerly_shared::storage::testing::InMemoryStorage;
use ledgerly_shared::web::{CurrentUser, render};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::scanner::{MalwareScanner, ScanError, ScanFuture, Verdict};
use crate::{Modules, Receipts};

pub(crate) const PDF: &[u8] = b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\n%%EOF\n";
const EXE: &[u8] = b"MZ\x90\0\x03\0\0\0\x04\0\0\0\xff\xff\0\0";

/// Keeps the type of every event, in order, and hands it on to the outbox, whose dispatcher delivers it to
/// the module's listeners as in the app.
#[derive(Debug)]
pub(crate) struct Recorded(pub Mutex<Vec<&'static str>>, pub Outbox);

impl Publisher for Recorded {
    fn publish<'a>(
        &'a self,
        connection: &'a mut sqlx::PgConnection,
        event: &'a dyn Event,
    ) -> Pin<Box<dyn Future<Output = sqlx::Result<()>> + Send + 'a>> {
        self.0.lock().unwrap().push(event.event_type());
        self.1.publish(connection, event)
    }
}

pub(crate) struct Reply {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or_default()
    }
}

/// What the fake scanner answers. Java: `FakeMalwareScanner`, which finds the EICAR string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Behaviour {
    /// Infected if the content contains `EICAR`, clean otherwise.
    Real,
    /// The scanner cannot be reached.
    Unavailable,
}

/// Records the content of every attempt.
#[derive(Debug)]
pub(crate) struct FakeScanner {
    behaviour: Mutex<Behaviour>,
    pub attempts: Mutex<Vec<Vec<u8>>>,
}

impl FakeScanner {
    pub(crate) fn set(&self, behaviour: Behaviour) {
        *self.behaviour.lock().unwrap() = behaviour;
    }

    pub(crate) fn attempts(&self) -> Vec<Vec<u8>> {
        self.attempts.lock().unwrap().clone()
    }
}

impl MalwareScanner for FakeScanner {
    fn scan<'a>(&'a self, content: &'a mut ledgerly_shared::storage::ObjectReader) -> ScanFuture<'a> {
        Box::pin(async move {
            let mut bytes = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(content, &mut bytes).await.unwrap();
            self.attempts.lock().unwrap().push(bytes.clone());
            match *self.behaviour.lock().unwrap() {
                Behaviour::Unavailable => Err(ScanError::Unreachable(std::io::Error::other("down"))),
                Behaviour::Real if bytes.windows(5).any(|w| w == b"EICAR") => Ok(Verdict::Infected {
                    signature: "Eicar-Test-Signature".to_owned(),
                }),
                Behaviour::Real => Ok(Verdict::Clean),
            }
        })
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.dispatcher.abort();
    }
}

/// A team with a claimant and a colleague (both MEMBER) and an APPROVER, as Java's test sets it up.
pub(crate) struct World {
    database: TestDatabase,
    router: Router,
    pub storage: InMemoryStorage,
    pub events: Arc<Recorded>,
    pub scanner: Arc<FakeScanner>,
    pub outbox: Outbox,
    dispatcher: tokio::task::JoinHandle<()>,
    pub listeners: Vec<Arc<dyn Listener>>,
    pub clock: Arc<dyn Clock>,
    pub team: TeamId,
    pub claimant: UserId,
    pub colleague: UserId,
    pub approver: UserId,
}

impl World {
    pub(crate) async fn new() -> Self {
        Self::with_limit(Limits::default().max_upload_size).await
    }

    pub(crate) async fn with_limit(max_upload_size: u64) -> Self {
        let database = TestDatabase::new().await.unwrap();
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 16, 12, 0, 0).unwrap()));
        let outbox = Outbox::new(database.pool().clone(), clock.clone());
        let events = Arc::new(Recorded(Mutex::default(), outbox.clone()));
        let publisher: Arc<dyn Publisher> = events.clone();
        let storage = InMemoryStorage::default();
        let objects: Arc<dyn ObjectStorage> = Arc::new(storage.clone());
        let limits = Limits {
            max_upload_size,
            ..Limits::default()
        };
        let scanner = Arc::new(FakeScanner {
            behaviour: Mutex::new(Behaviour::Real),
            attempts: Mutex::new(Vec::new()),
        });
        let receipts = Receipts::assemble(
            Modules {
                pool: database.pool(),
                clock: &clock,
                events: &publisher,
                expenses: ExpensesFacade::default(),
            },
            &objects,
            scanner.clone(),
            [std::time::Duration::ZERO; 3],
            &limits,
        );
        let listeners = receipts.listeners();
        outbox.subscribe(listeners.clone()).unwrap();
        let dispatcher = outbox.start();
        let router = Router::new()
            .fallback_service(receipts.routes().into_service())
            .layer(axum::middleware::from_fn(render));
        let world = Self {
            database,
            router,
            storage,
            events,
            scanner,
            outbox,
            dispatcher,
            listeners,
            clock,
            team: TeamId::new_v7(),
            claimant: UserId::new_v7(),
            colleague: UserId::new_v7(),
            approver: UserId::new_v7(),
        };
        sqlx::query("insert into teams (id, name, created_at) values ($1, 'Team', now())")
            .bind(world.team.as_uuid())
            .execute(world.pool())
            .await
            .unwrap();
        for (user, role) in [
            (world.claimant, "MEMBER"),
            (world.colleague, "MEMBER"),
            (world.approver, "APPROVER"),
        ] {
            world.member(world.team, user, role).await;
        }
        world
    }

    /// The process dies: the dispatcher stops, deliveries in progress are abandoned.
    pub(crate) fn kill_dispatcher(&self) {
        self.dispatcher.abort();
    }

    /// A new process starts: a fresh outbox with the same listeners delivers what is open.
    pub(crate) fn restart(&self) -> tokio::task::JoinHandle<()> {
        let outbox = Outbox::new(self.pool().clone(), self.clock.clone());
        outbox.subscribe(self.listeners.clone()).unwrap();
        outbox.start()
    }

    pub(crate) async fn open_publications(&self) -> Vec<(String, String)> {
        sqlx::query_as(
            "select listener_id, coalesce(status, '') from event_publication where completion_date is null \
             order by publication_date",
        )
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    pub(crate) fn pool(&self) -> &PgPool {
        self.database.pool()
    }

    pub(crate) async fn member(&self, team: TeamId, user: UserId, role: &str) {
        sqlx::query("insert into memberships (team_id, user_id, role, joined_at) values ($1, $2, $3, now())")
            .bind(team.as_uuid())
            .bind(user.as_uuid())
            .bind(role)
            .execute(self.pool())
            .await
            .unwrap();
    }

    /// An expense of the claimant in `status`, inserted with SQL.
    pub(crate) async fn expense(&self, status: &str) -> ExpenseId {
        let id = ExpenseId::new_v7();
        sqlx::query(
            "insert into expenses (id, team_id, claimant_id, amount, currency, category, incurred_on, merchant, \
             note, status, version, created_at, updated_at) \
             values ($1, $2, $3, 12.5, 'EUR', 'MEALS', '2026-11-01', 'Cafe', null, $4, 0, now(), now())",
        )
        .bind(id.as_uuid())
        .bind(self.team.as_uuid())
        .bind(self.claimant.as_uuid())
        .bind(status)
        .execute(self.pool())
        .await
        .unwrap();
        id
    }

    pub(crate) fn receipts(&self, expense: ExpenseId) -> String {
        format!("/api/v1/teams/{}/expenses/{expense}/receipts", self.team)
    }

    pub(crate) async fn send(&self, request: Request<HttpBody>, caller: UserId) -> Reply {
        let mut request = request;
        request.extensions_mut().insert(CurrentUser::new(caller));
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply { status, headers, body }
    }

    pub(crate) async fn upload(&self, expense: ExpenseId, caller: UserId, name: &str, content: &[u8]) -> Reply {
        self.send(multipart(&self.receipts(expense), "file", name, content), caller)
            .await
    }

    pub(crate) async fn list(&self, expense: ExpenseId, caller: UserId) -> Reply {
        let request = Request::get(self.receipts(expense)).body(HttpBody::empty()).unwrap();
        self.send(request, caller).await
    }

    pub(crate) async fn rows(&self) -> i64 {
        sqlx::query_scalar("select count(*) from receipts")
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    pub(crate) fn events(&self) -> Vec<&'static str> {
        self.events.0.lock().unwrap().clone()
    }

    /// The receipt's status and key once `done` says the scan task has got far enough, within 10 seconds.
    pub(crate) async fn settled(&self, receipt: &str, done: impl Fn(&str) -> bool) -> (String, String) {
        let id: Uuid = receipt.parse().unwrap();
        for _ in 0..200 {
            let (status, key): (String, String) =
                sqlx::query_as("select scan_status, storage_key from receipts where id = $1")
                    .bind(id)
                    .fetch_one(self.pool())
                    .await
                    .unwrap();
            if done(&status) {
                return (status, key);
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("the scan of {receipt} did not settle");
    }
}

/// A `multipart/form-data` POST with one file part.
pub(crate) fn multipart(path: &str, part: &str, name: &str, content: &[u8]) -> Request<HttpBody> {
    let mut body = format!(
        "--b0undary\r\nContent-Disposition: form-data; name=\"{part}\"; filename=\"{name}\"\r\n\
         Content-Type: application/pdf\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(b"\r\n--b0undary--\r\n");
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("content-type", "multipart/form-data; boundary=b0undary")
        .body(HttpBody::from(body))
        .unwrap()
}

#[tokio::test]
async fn an_upload_is_stored_under_a_random_key_and_listed_as_pending() {
    let world = World::new().await;
    // The outbox delivers the upload to the scanner as soon as it commits; a scanner that cannot answer keeps
    // the receipt pending for the listing below, however fast the delivery is.
    world.scanner.set(Behaviour::Unavailable);
    let expense = world.expense("DRAFT").await;

    let reply = world.upload(expense, world.claimant, "lunch receipt.pdf", PDF).await;

    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.body);
    assert_eq!(
        reply.headers["location"],
        format!("/api/v1/teams/{}/expenses/{expense}/receipts", world.team).as_str()
    );
    let body = &reply.body;
    assert_eq!(body["expenseId"], expense.to_string());
    assert_eq!(body["status"], "PENDING_SCAN");
    assert_eq!(body["detectedType"], "application/pdf");
    assert_eq!(body["sizeBytes"], PDF.len());
    assert_eq!(body["originalName"], "lunch receipt.pdf");
    assert_eq!(body["possibleDuplicate"], false);
    let keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "detectedType",
            "expenseId",
            "id",
            "originalName",
            "possibleDuplicate",
            "sizeBytes",
            "status"
        ]
    );

    let (key, sha256): (String, String) = sqlx::query_as("select storage_key, sha256 from receipts where id = $1")
        .bind(body["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(world.pool())
        .await
        .unwrap();
    assert!(key.strip_prefix("receipts/").unwrap().parse::<Uuid>().is_ok(), "{key}");
    assert_eq!(world.storage.keys(), std::slice::from_ref(&key));
    let object = world.storage.get(&key).unwrap();
    assert_eq!(object.content, PDF);
    assert_eq!(object.content_type, "application/pdf");
    assert_eq!(sha256.len(), 64);
    assert!(sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_eq!(world.events()[0], "ledgerly-rust:receipts.ReceiptUploaded");

    let list = world.list(expense, world.claimant).await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.body.as_array().unwrap().len(), 1);
    assert_eq!(list.body[0], reply.body);
}

/// The permission is asked before a byte of the body is read: a body that is not even multipart gets the
/// expenses module's answer, not the parser's.
#[tokio::test]
async fn whoever_may_not_upload_is_refused_before_the_body_is_read() {
    let world = World::new().await;
    let draft = world.expense("DRAFT").await;
    let submitted = world.expense("SUBMITTED").await;
    let garbage = |expense| {
        Request::post(world.receipts(expense))
            .header("content-type", "multipart/form-data; boundary=b0undary")
            .body(HttpBody::from("not multipart at all"))
            .unwrap()
    };
    let outsider = UserId::new_v7();

    let cases = [
        (draft, world.approver, StatusCode::FORBIDDEN, "FORBIDDEN"),
        (draft, world.colleague, StatusCode::NOT_FOUND, "NOT_FOUND"),
        (draft, outsider, StatusCode::NOT_FOUND, "NOT_FOUND"),
        (submitted, world.claimant, StatusCode::CONFLICT, "EXPENSE_NOT_DRAFT"),
        (ExpenseId::new_v7(), world.claimant, StatusCode::NOT_FOUND, "NOT_FOUND"),
    ];
    for (expense, caller, status, code) in cases {
        let reply = world.send(garbage(expense), caller).await;
        assert_eq!((reply.status, reply.code()), (status, code), "{}", reply.body);
    }
    assert_eq!(
        world.send(garbage(draft), world.approver).await.body["detail"],
        "Only the claimant may change receipts"
    );
    assert!(world.storage.keys().is_empty());
}

#[tokio::test]
async fn a_clean_file_is_accepted_once_scanned() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;

    let reply = world.upload(expense, world.claimant, "a.pdf", PDF).await;

    let receipt = reply.body["id"].as_str().unwrap();
    let (status, key) = world.settled(receipt, |status| status != "PENDING_SCAN").await;
    assert_eq!(status, "ACCEPTED");
    assert!(key.starts_with("receipts/"));
    assert_eq!(world.scanner.attempts(), [PDF.to_vec()]);
    assert_eq!(
        world.events(),
        [
            "ledgerly-rust:receipts.ReceiptUploaded",
            "ledgerly-rust:receipts.ReceiptAccepted"
        ]
    );
}

#[tokio::test]
async fn an_infected_file_is_quarantined_and_its_object_moved() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    let infected = [PDF, b"EICAR".as_slice()].concat();

    let reply = world.upload(expense, world.claimant, "a.pdf", &infected).await;

    let receipt = reply.body["id"].as_str().unwrap();
    let (status, key) = world.settled(receipt, |status| status != "PENDING_SCAN").await;
    assert_eq!(status, "QUARANTINED");
    let suffix = key.strip_prefix("quarantine/").unwrap();
    assert!(suffix.parse::<Uuid>().is_ok(), "{key}");
    // The move follows the row's change.
    for _ in 0..100 {
        if world.storage.keys() == [key.clone()] {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(world.storage.keys(), std::slice::from_ref(&key));
    assert_eq!(world.storage.get(&key).unwrap().content, infected);
    assert_eq!(
        world.events(),
        [
            "ledgerly-rust:receipts.ReceiptUploaded",
            "ledgerly-rust:receipts.ReceiptQuarantined"
        ]
    );
}

/// Four attempts, each with the whole file (Java hands the half-read stream to its retries), then the receipt
/// stays pending.
#[tokio::test]
async fn while_the_scanner_is_down_the_receipt_stays_pending_after_four_whole_attempts() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    world.scanner.set(Behaviour::Unavailable);

    let reply = world.upload(expense, world.claimant, "a.pdf", PDF).await;

    for _ in 0..200 {
        if world.scanner.attempts().len() == 4 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(world.scanner.attempts(), vec![PDF.to_vec(); 4]);
    let (status, _) = world.settled(reply.body["id"].as_str().unwrap(), |_| true).await;
    assert_eq!(status, "PENDING_SCAN");
    assert_eq!(world.events(), ["ledgerly-rust:receipts.ReceiptUploaded"]);
}

/// The plan's acceptance test for M9.2, and Java's `EventRegistryRecoveryTest`: the scanner is down, so the
/// receipt waits as `PENDING_SCAN` with an open publication; the application dies; after the restart the
/// upload is delivered again, the receipt becomes `ACCEPTED` and the publication is completed.
#[tokio::test]
async fn a_scan_that_never_finished_is_done_after_a_restart() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    world.scanner.set(Behaviour::Unavailable);
    let reply = world.upload(expense, world.claimant, "a.pdf", PDF).await;
    let receipt = reply.body["id"].as_str().unwrap().to_owned();
    for _ in 0..200 {
        if world.open_publications().await == [("ledgerly-rust:receipts.ScanReceipt".to_owned(), "FAILED".to_owned())] {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        world.open_publications().await,
        [("ledgerly-rust:receipts.ScanReceipt".to_owned(), "FAILED".to_owned())]
    );
    world.kill_dispatcher();

    world.scanner.set(Behaviour::Real);
    let restarted = world.restart();

    let (status, _) = world.settled(&receipt, |status| status != "PENDING_SCAN").await;
    assert_eq!(status, "ACCEPTED");
    for _ in 0..200 {
        if world.open_publications().await.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(world.open_publications().await.is_empty());
    restarted.abort();
}

/// Java: `ExpenseDeletedCleanup` listens to `ExpenseEvent.Deleted`: once the deletion is delivered, the
/// expense's receipts and files are gone, and nobody else's.
#[tokio::test]
async fn an_expense_deleted_event_removes_its_receipts() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    let other = world.expense("DRAFT").await;
    world.accepted(expense, "a.pdf", PDF).await;
    let kept = world.accepted(other, "c.pdf", PDF).await;

    let deleted = ledgerly_expenses::ExpenseEvent {
        event_id: Uuid::now_v7(),
        team_id: world.team,
        expense_id: expense,
        actor_id: world.claimant,
        occurred_at: Utc::now(),
        kind: ledgerly_expenses::ExpenseEventKind::Deleted,
    };
    let mut transaction = world.pool().begin().await.unwrap();
    world.outbox.publish(&mut transaction, &deleted).await.unwrap();
    transaction.commit().await.unwrap();

    for _ in 0..200 {
        if world.rows().await == 1 && world.open_publications().await.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(world.rows().await, 1);
    assert_eq!(world.storage.keys(), [world.key_of(&kept).await]);
    assert!(world.open_publications().await.is_empty());
}

#[tokio::test]
async fn the_upload_must_be_multipart_with_a_file_part() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;

    let json = Request::post(world.receipts(expense))
        .header("content-type", "application/json")
        .body(HttpBody::from("{}"))
        .unwrap();
    let reply = world.send(json, world.claimant).await;
    assert_eq!(reply.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(reply.headers["accept"], "multipart/form-data");
    assert_eq!(
        reply.body["detail"],
        "Content-Type 'application/json' is not supported."
    );

    let other_part = world
        .send(
            multipart(&world.receipts(expense), "other", "x.pdf", PDF),
            world.claimant,
        )
        .await;
    assert_eq!(other_part.status, StatusCode::BAD_REQUEST);
    assert_eq!(other_part.code(), "REQUEST_REJECTED");
    assert_eq!(other_part.body["detail"], "Required part 'file' is not present.");

    let broken = Request::post(world.receipts(expense))
        .header("content-type", "multipart/form-data; boundary=b0undary")
        .body(HttpBody::from("--b0undary\r\nno headers end"))
        .unwrap();
    let broken = world.send(broken, world.claimant).await;
    assert_eq!(broken.status, StatusCode::BAD_REQUEST);
    assert_eq!(broken.code(), "MALFORMED_REQUEST");
}

#[tokio::test]
async fn empty_and_unknown_files_are_refused_and_nothing_is_stored() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;

    let empty = world.upload(expense, world.claimant, "empty.pdf", b"").await;
    let exe = world.upload(expense, world.claimant, "invoice.pdf", EXE).await;
    let html = world
        .upload(
            expense,
            world.claimant,
            "a.png",
            b"<html><script>alert(1)</script></html>",
        )
        .await;

    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
    assert_eq!(empty.code(), "VALIDATION_FAILED");
    assert_eq!(
        empty.body["errors"],
        serde_json::json!([{"field": "file", "message": "The file is empty"}])
    );
    for refused in [&exe, &html] {
        assert_eq!(refused.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(refused.code(), "UNSUPPORTED_MEDIA_TYPE");
        assert_eq!(refused.body["detail"], "Only PDF, JPEG and PNG files are accepted");
    }
    assert!(world.storage.keys().is_empty());
    assert_eq!(world.rows().await, 0);
    assert!(world.events().is_empty());
}

#[tokio::test]
async fn the_same_file_twice_is_409_on_one_expense_and_flagged_on_another() {
    let world = World::new().await;
    let first = world.expense("DRAFT").await;
    let second = world.expense("DRAFT").await;
    assert_eq!(
        world.upload(first, world.claimant, "a.pdf", PDF).await.status,
        StatusCode::ACCEPTED
    );

    let again = world.upload(first, world.claimant, "b.pdf", PDF).await;
    let sibling = world.upload(second, world.claimant, "c.pdf", PDF).await;

    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.code(), "RECEIPT_DUPLICATE");
    assert_eq!(again.body["detail"], "This file is already attached to the expense");
    assert_eq!(sibling.status, StatusCode::ACCEPTED);
    assert_eq!(sibling.body["possibleDuplicate"], true);
    assert_eq!(world.storage.keys().len(), 2);
}

/// Java's `CappedInputStream` at `ledgerly.limits.max-upload-size`, below Spring's 10 MiB per part: its own
/// message, unless the part is also over Spring's limit, which Spring refuses first.
#[tokio::test]
async fn the_configured_cap_has_its_own_message_unless_spring_refused_first() {
    let world = World::with_limit(1024 * 1024).await;
    let expense = world.expense("DRAFT").await;
    let mut over_the_cap = PDF.to_vec();
    over_the_cap.resize(1024 * 1024 + 1, 0);
    let mut over_both = PDF.to_vec();
    over_both.resize(10 * 1024 * 1024 + 1, 0);

    let capped = world.upload(expense, world.claimant, "a.pdf", &over_the_cap).await;
    let refused = world.upload(expense, world.claimant, "b.pdf", &over_both).await;

    assert_eq!(capped.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(capped.body["detail"], "The file is larger than 1 MB");
    assert_eq!(refused.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(refused.body["detail"], "The upload is larger than the limit");
    assert_eq!(world.rows().await, 0);
}

#[tokio::test]
async fn a_store_that_is_down_is_a_500_and_leaves_no_receipt() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    world.storage.set_unavailable(true);

    let reply = world.upload(expense, world.claimant, "a.pdf", PDF).await;

    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.code(), "INTERNAL_ERROR");
    assert_eq!(world.rows().await, 0);
}

#[tokio::test]
async fn the_list_is_visible_like_the_expense_and_oldest_first() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    world.upload(expense, world.claimant, "first.pdf", PDF).await;
    world
        .upload(expense, world.claimant, "second.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        .await;

    let mine = world.list(expense, world.claimant).await;
    let approver = world.list(expense, world.approver).await;
    let colleague = world.list(expense, world.colleague).await;

    let names: Vec<&str> = mine
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["originalName"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["first.pdf", "second.png"]);
    assert_eq!(approver.status, StatusCode::OK);
    assert_eq!(colleague.status, StatusCode::NOT_FOUND);
}

impl World {
    /// A receipt of the claimant's draft, scanned clean.
    async fn accepted(&self, expense: ExpenseId, name: &str, content: &[u8]) -> String {
        let reply = self.upload(expense, self.claimant, name, content).await;
        let receipt = reply.body["id"].as_str().unwrap().to_owned();
        let (status, _) = self.settled(&receipt, |status| status != "PENDING_SCAN").await;
        assert_eq!(status, "ACCEPTED");
        receipt
    }

    async fn download(&self, expense: ExpenseId, receipt: &str, caller: UserId) -> Reply {
        let path = format!("{}/{receipt}/download", self.receipts(expense));
        self.send(Request::get(path).body(HttpBody::empty()).unwrap(), caller)
            .await
    }

    async fn delete(&self, expense: ExpenseId, receipt: &str, caller: UserId) -> Reply {
        let path = format!("{}/{receipt}", self.receipts(expense));
        self.send(Request::delete(path).body(HttpBody::empty()).unwrap(), caller)
            .await
    }

    async fn key_of(&self, receipt: &str) -> String {
        sqlx::query_scalar("select storage_key from receipts where id = $1")
            .bind(receipt.parse::<Uuid>().unwrap())
            .fetch_one(self.pool())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn an_accepted_receipt_is_downloaded_through_a_five_minute_link_under_its_sanitised_name() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    let receipt = world.accepted(expense, "../../etc/passwd", PDF).await;

    let reply = world.download(expense, &receipt, world.claimant).await;

    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.headers["cache-control"], "no-store");
    let key = world.key_of(&receipt).await;
    assert_eq!(reply.body["url"], format!("memory://{key}"));
    // The fixed clock is at 12:00:00.
    assert_eq!(reply.body["expiresAt"], "2026-11-16T12:05:00Z");
    let presigned = world.storage.presigned();
    assert_eq!(presigned.len(), 1);
    assert_eq!(presigned[0].key, key);
    assert_eq!(presigned[0].download_name, "passwd.pdf");
    assert_eq!(presigned[0].content_type, "application/pdf");
    assert_eq!(presigned[0].ttl, std::time::Duration::from_secs(300));
    assert_eq!(
        world.download(expense, &receipt, world.approver).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn pending_quarantined_and_unreachable_receipts_are_404() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    let sibling = world.expense("DRAFT").await;
    let accepted = world.accepted(expense, "a.pdf", PDF).await;
    let infected = world
        .upload(expense, world.claimant, "b.pdf", &[PDF, b"EICAR".as_slice()].concat())
        .await;
    let infected = infected.body["id"].as_str().unwrap().to_owned();
    world.settled(&infected, |status| status == "QUARANTINED").await;
    world.scanner.set(Behaviour::Unavailable);
    let pending = world
        .upload(expense, world.claimant, "c.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        .await;
    let pending = pending.body["id"].as_str().unwrap().to_owned();

    for (expense, receipt, caller) in [
        (expense, infected.as_str(), world.claimant),
        (expense, infected.as_str(), world.approver),
        (expense, pending.as_str(), world.claimant),
        (expense, accepted.as_str(), world.colleague),
        (sibling, accepted.as_str(), world.claimant),
        (expense, &Uuid::new_v4().to_string(), world.claimant),
    ] {
        let reply = world.download(expense, receipt, caller).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{receipt}");
        assert_eq!(reply.code(), "NOT_FOUND");
    }
    assert!(world.storage.presigned().is_empty());
}

#[tokio::test]
async fn the_claimant_deletes_a_receipt_of_a_draft_with_its_object() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    let receipt = world.accepted(expense, "a.pdf", PDF).await;
    let key = world.key_of(&receipt).await;

    let deleted = world.delete(expense, &receipt, world.claimant).await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(world.rows().await, 0);
    assert!(world.storage.get(&key).is_none());
    assert_eq!(world.events().last(), Some(&"ledgerly-rust:receipts.ReceiptDeleted"));
    assert_eq!(
        world.delete(expense, &receipt, world.claimant).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn only_the_claimant_deletes_and_only_from_a_draft() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    let receipt = world.accepted(expense, "a.pdf", PDF).await;
    let sibling = world.expense("DRAFT").await;

    assert_eq!(
        world.delete(expense, &receipt, world.approver).await.code(),
        "FORBIDDEN"
    );
    assert_eq!(
        world.delete(expense, &receipt, world.colleague).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        world.delete(sibling, &receipt, world.claimant).await.status,
        StatusCode::NOT_FOUND
    );
    sqlx::query("update expenses set status = 'SUBMITTED' where id = $1")
        .bind(expense.as_uuid())
        .execute(world.pool())
        .await
        .unwrap();
    assert_eq!(
        world.delete(expense, &receipt, world.claimant).await.code(),
        "EXPENSE_NOT_DRAFT"
    );
    assert_eq!(world.rows().await, 1);
}

#[tokio::test]
async fn deleting_an_expense_removes_its_receipts_and_files_but_no_others() {
    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    let other = world.expense("DRAFT").await;
    world.accepted(expense, "a.pdf", PDF).await;
    world.accepted(expense, "b.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").await;
    let kept = world.accepted(other, "c.jpg", &[0xFF, 0xD8, 0xFF, 0xE0]).await;
    let receipts = Receipts::assemble(
        Modules {
            pool: world.pool(),
            clock: &(Arc::new(FixedClock::at(Utc::now())) as Arc<dyn Clock>),
            events: &(world.events.clone() as Arc<dyn Publisher>),
            expenses: ExpensesFacade::default(),
        },
        &(Arc::new(world.storage.clone()) as Arc<dyn ObjectStorage>),
        world.scanner.clone(),
        [std::time::Duration::ZERO; 3],
        &Limits::default(),
    );

    receipts.cleanup.execute(world.team, expense).await.unwrap();

    assert_eq!(world.rows().await, 1);
    assert_eq!(world.storage.keys(), [world.key_of(&kept).await]);
}

#[tokio::test]
async fn the_submit_rule_counts_accepted_and_pending_receipts_of_the_expense() {
    use ledgerly_expenses::ReceiptSummary;

    let world = World::new().await;
    let expense = world.expense("DRAFT").await;
    let other = world.expense("DRAFT").await;
    world.accepted(expense, "a.pdf", PDF).await;
    world.accepted(other, "b.pdf", PDF).await;
    world.scanner.set(Behaviour::Unavailable);
    world
        .upload(expense, world.claimant, "c.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        .await;
    let mut connection = world.pool().acquire().await.unwrap();

    let summary = Receipts::checks()
        .summary_for(&mut connection, world.team, expense)
        .await
        .unwrap();
    let elsewhere = Receipts::checks()
        .summary_for(&mut connection, TeamId::new_v7(), expense)
        .await
        .unwrap();

    assert_eq!(summary, ReceiptSummary::new(1, 1));
    assert_eq!(elsewhere, ReceiptSummary::new(0, 0));
}
