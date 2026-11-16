//! The imports endpoints and the job in-process (axum `oneshot`) against real Postgres, with the file store in
//! memory, a clock the tests move, and an expenses facade a test can make fail.
//!
//! Java: the parts of `ImportApiTest` that need a clock or a failing facade (`NOT_PARITY.md`), and
//! `ImportSkipListenerTest`. The rest runs black-box in the parity suite.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body as HttpBody;
use axum::http::{Method, Request, StatusCode};
use chrono::{TimeDelta, TimeZone, Utc};
use ledgerly_expenses::{ExpenseRefusal, ExpensesFacade, NewExpense, ValidationProblem};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::config::Limits;
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::events::{Discard, Outbox};
use ledgerly_shared::id::{ExpenseId, ImportId, TeamId, UserId};
use ledgerly_shared::storage::testing::InMemoryStorage;
use ledgerly_shared::web::{CurrentUser, render};
use ledgerly_teams::TeamsFacade;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

use crate::Imports;
use crate::application::{Context, Drafts};

/// The real facade, except that the `fail_on`-th call of `create` fails, like a database outage. Java:
/// `ImportApiTest.FlakyFacade`.
#[derive(Debug)]
struct Flaky {
    real: ExpensesFacade,
    calls: AtomicUsize,
    fail_on: AtomicUsize,
}

impl Drafts for Flaky {
    fn validate(&self, expense: &NewExpense) -> Option<ValidationProblem> {
        self.real.validate(expense)
    }

    fn create<'a>(
        &'a self,
        connection: &'a mut PgConnection,
        team: TeamId,
        claimant: UserId,
        drafts: &'a [NewExpense],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ExpenseId>, ExpenseRefusal>> + Send + 'a>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.fail_on.load(Ordering::SeqCst) {
            // Not a teammate any more: a refusal, as good as an outage for the job.
            return Box::pin(self.real.create_drafts(connection, TeamId::new_v7(), claimant, drafts));
        }
        Box::pin(self.real.create_drafts(connection, team, claimant, drafts))
    }
}

struct World {
    database: TestDatabase,
    clock: Arc<FixedClock>,
    storage: InMemoryStorage,
    flaky: Arc<Flaky>,
    imports: Imports,
    dispatcher: tokio::task::JoinHandle<()>,
    router: Router,
    team: TeamId,
    importer: UserId,
    owner: UserId,
    approver: UserId,
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or_default()
    }

    fn id(&self) -> ImportId {
        ImportId::from_uuid(self.body["id"].as_str().unwrap().parse().unwrap())
    }
}

const HEADER: &str = "date,merchant,amount,currency,category,note";

fn csv(rows: &[String]) -> String {
    format!("{HEADER}\n{}\n", rows.join("\n"))
}

fn good_row(n: usize) -> String {
    format!("2026-01-{:02},Merchant {n},12.50,EUR,MEALS,", 1 + n % 28)
}

impl Drop for World {
    fn drop(&mut self) {
        self.dispatcher.abort();
    }
}

impl World {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let clock = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 16, 12, 0, 0).unwrap()));
        let shared: Arc<dyn Clock> = clock.clone();
        let limits = Limits::default();
        let storage = InMemoryStorage::default();
        let flaky = Arc::new(Flaky {
            real: ExpensesFacade::new(TeamsFacade, shared.clone(), Arc::new(Discard), &limits),
            calls: AtomicUsize::new(0),
            fail_on: AtomicUsize::new(0),
        });
        let outbox = Outbox::new(database.pool().clone(), shared.clone());
        let imports = Imports::with(Context {
            pool: database.pool().clone(),
            clock: shared,
            events: Arc::new(outbox.clone()),
            storage: Arc::new(storage.clone()),
            teams: TeamsFacade,
            drafts: flaky.clone(),
            limits,
        });
        outbox.subscribe(imports.listeners()).unwrap();
        let dispatcher = outbox.start();
        let router = Router::new()
            .fallback_service(imports.routes().into_service())
            .layer(axum::middleware::from_fn(render));
        let world = Self {
            database,
            clock,
            storage,
            flaky,
            imports,
            dispatcher,
            router,
            team: TeamId::new_v7(),
            importer: UserId::new_v7(),
            owner: UserId::new_v7(),
            approver: UserId::new_v7(),
        };
        world.add_team(world.team).await;
        world.member(world.team, world.importer, "MEMBER").await;
        world.member(world.team, world.owner, "OWNER").await;
        world.member(world.team, world.approver, "APPROVER").await;
        world
    }

    fn pool(&self) -> &PgPool {
        self.database.pool()
    }

    async fn add_team(&self, team: TeamId) {
        sqlx::query("insert into teams (id, name, created_at) values ($1, 'Team', now())")
            .bind(team.as_uuid())
            .execute(self.pool())
            .await
            .unwrap();
    }

    async fn member(&self, team: TeamId, user: UserId, role: &str) {
        sqlx::query("insert into memberships (team_id, user_id, role, joined_at) values ($1, $2, $3, now())")
            .bind(team.as_uuid())
            .bind(user.as_uuid())
            .bind(role)
            .execute(self.pool())
            .await
            .unwrap();
    }

    async fn send(&self, request: Request<HttpBody>, caller: UserId) -> Reply {
        let mut request = request;
        request.extensions_mut().insert(CurrentUser::new(caller));
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply { status, headers, body }
    }

    async fn upload_to(&self, team: TeamId, caller: UserId, content: &[u8]) -> Reply {
        let mut body = b"--b0undary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"e.csv\"\r\n\
                         Content-Type: text/csv\r\n\r\n"
            .to_vec();
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n--b0undary--\r\n");
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/teams/{team}/imports"))
            .header("content-type", "multipart/form-data; boundary=b0undary")
            .body(HttpBody::from(body))
            .unwrap();
        self.send(request, caller).await
    }

    async fn upload(&self, content: &str) -> Reply {
        self.upload_to(self.team, self.importer, content.as_bytes()).await
    }

    async fn get(&self, id: ImportId, caller: UserId) -> Reply {
        let request = Request::get(format!("/api/v1/teams/{}/imports/{id}", self.team))
            .body(HttpBody::empty())
            .unwrap();
        self.send(request, caller).await
    }

    /// Polls until the import is COMPLETED or FAILED, for up to 30 seconds.
    async fn finished(&self, id: ImportId) -> Value {
        for _ in 0..600 {
            let reply = self.get(id, self.importer).await;
            if reply.body["status"] == "COMPLETED" || reply.body["status"] == "FAILED" {
                return reply.body;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("import {id} did not finish");
    }

    async fn expenses(&self) -> i64 {
        sqlx::query_scalar("select count(*) from expenses where team_id = $1")
            .bind(self.team.as_uuid())
            .fetch_one(self.pool())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn an_import_creates_drafts_chunk_by_chunk_and_deletes_its_file() {
    let world = World::new().await;
    let rows: Vec<String> = (0..250).map(good_row).collect();

    let queued = world.upload(&csv(&rows)).await;

    assert_eq!(queued.status, StatusCode::ACCEPTED, "{}", queued.body);
    let id = queued.id();
    assert_eq!(
        queued.headers["location"],
        format!("/api/v1/teams/{}/imports/{id}", world.team).as_str()
    );
    assert_eq!(queued.body["totalRows"], 250);
    let keys: Vec<&str> = queued.body.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "errors",
            "errorsTruncated",
            "id",
            "importedRows",
            "skippedRows",
            "status",
            "totalRows"
        ]
    );
    let done = world.finished(id).await;
    assert_eq!(done["status"], "COMPLETED");
    assert_eq!(done["importedRows"], 250);
    assert_eq!(world.expenses().await, 250);
    assert_eq!(world.flaky.calls.load(Ordering::SeqCst), 3, "chunks of 100");
    assert!(
        world.storage.keys().is_empty(),
        "the file goes once the import completed"
    );
    let (status, original_name, started, finished): (String, String, bool, bool) = sqlx::query_as(
        "select status, original_name, started_at is not null, finished_at is not null from import_jobs where id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(world.pool())
    .await
    .unwrap();
    assert_eq!(
        (status.as_str(), original_name.as_str(), started, finished),
        ("COMPLETED", "e.csv", true, true)
    );
}

#[tokio::test]
async fn row_errors_carry_javas_codes_and_messages() {
    let world = World::new().await;
    let rows = [
        "2026-01-05,,5.00,EUR,MEALS,".to_owned(),
        "2999-01-01,Cafe,5.00,EUR,MEALS,".to_owned(),
        "2026-01-05,Cafe,5.00,XXX,MEALS,".to_owned(),
        "# a comment line is skipped, but counted".to_owned(),
        good_row(1),
    ];

    let done = world.finished(world.upload(&csv(&rows)).await.id()).await;

    assert_eq!(done["importedRows"], 1);
    assert_eq!(done["skippedRows"], 3);
    let errors: Vec<(u64, &str, &str)> = done["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["line"].as_u64().unwrap(),
                e["code"].as_str().unwrap(),
                e["message"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(errors[0], (2, "VALIDATION_FAILED", "merchant: Merchant is required"));
    assert_eq!(
        errors[1],
        (
            3,
            "VALIDATION_FAILED",
            "incurredOn: incurredOn must not be in the future"
        )
    );
    assert_eq!(errors[2].0, 4);
    assert_eq!(errors[2].1, "AMOUNT_INVALID");
}

/// Java: `ImportSkipListenerTest`: a message is cut at 500 characters, and the same line twice is recorded
/// once.
#[tokio::test]
async fn a_row_error_is_recorded_once_and_cut_at_500_characters() {
    let world = World::new().await;
    let id = world.upload(&csv(&[good_row(1)])).await.id();
    world.finished(id).await;
    let mut connection = world.pool().acquire().await.unwrap();

    crate::persistence::add_error(&mut connection, id, 7, "X", &"m".repeat(600))
        .await
        .unwrap();
    crate::persistence::add_error(&mut connection, id, 7, "Y", "again")
        .await
        .unwrap();

    let errors = crate::persistence::errors(&mut connection, world.team, id, 10)
        .await
        .unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "X");
    assert_eq!(errors[0].message.len(), 500);
}

/// Java: `ImportApiTest.aFailedImportRestartsFromTheLastCommittedChunkWithoutDuplicates`.
#[tokio::test]
async fn a_failed_import_restarts_after_the_last_committed_chunk_without_duplicates() {
    let world = World::new().await;
    let rows: Vec<String> = (0..450).map(good_row).collect();
    world.flaky.fail_on.store(3, Ordering::SeqCst);

    let id = world.upload(&csv(&rows)).await.id();
    let failed = world.finished(id).await;

    assert_eq!(failed["status"], "FAILED");
    assert_eq!(failed["importedRows"], 200);
    assert_eq!(world.expenses().await, 200);
    assert_eq!(world.storage.keys().len(), 1, "a failed import keeps its file");

    world.flaky.fail_on.store(0, Ordering::SeqCst);
    assert!(world.imports.restart(world.team, id).await);
    let done = world.finished(id).await;
    assert_eq!(done["status"], "COMPLETED");
    assert_eq!(done["importedRows"], 450);
    let distinct: i64 = sqlx::query_scalar("select count(distinct merchant) from expenses where team_id = $1")
        .bind(world.team.as_uuid())
        .fetch_one(world.pool())
        .await
        .unwrap();
    assert_eq!((world.expenses().await, distinct), (450, 450));
}

/// Java: `ImportApiTest.errorsAlreadyReportedBeforeAFailureAreNotDuplicatedOnRestart`.
#[tokio::test]
async fn errors_reported_before_a_failure_are_not_duplicated_on_restart() {
    let world = World::new().await;
    let mut rows: Vec<String> = (0..250).map(good_row).collect();
    rows[5] = "2026-01-05,Cafe,abc,EUR,MEALS,".to_owned();
    world.flaky.fail_on.store(2, Ordering::SeqCst);
    let id = world.upload(&csv(&rows)).await.id();
    assert_eq!(world.finished(id).await["status"], "FAILED");

    world.flaky.fail_on.store(0, Ordering::SeqCst);
    world.imports.restart(world.team, id).await;
    let done = world.finished(id).await;

    assert_eq!(done["status"], "COMPLETED");
    assert_eq!(done["skippedRows"], 1);
    assert_eq!(done["importedRows"], 249);
    assert_eq!(done["errors"][0]["line"], 7);
    assert_eq!(world.expenses().await, 249);
}

/// Java: `aCompletedImportCannotBeRestarted` and `deliveringTheQueuedEventAgainDoesNotRunACompletedImportTwice`.
#[tokio::test]
async fn a_completed_import_is_neither_restarted_nor_run_again() {
    let world = World::new().await;
    let id = world.upload(&csv(&[good_row(1), good_row(2), good_row(3)])).await.id();
    world.finished(id).await;

    assert!(!world.imports.restart(world.team, id).await);
    assert!(!world.imports.restart(world.team, ImportId::new_v4()).await);
    world.imports.runner.run(world.team, id).await.unwrap();

    assert_eq!(world.expenses().await, 3);
}

/// Decided after M8: the process died while the job ran, so the import is left `RUNNING` and its `ImportQueued`
/// publication open. The next start delivers it again, and the job resumes after the last committed chunk.
/// Java leaves such an import `RUNNING`.
#[tokio::test]
async fn an_import_left_running_by_a_stopped_process_resumes_at_the_next_start() {
    let world = World::new().await;
    let rows: Vec<String> = (0..450).map(good_row).collect();
    world.flaky.fail_on.store(3, Ordering::SeqCst);
    let id = world.upload(&csv(&rows)).await.id();
    assert_eq!(world.finished(id).await["status"], "FAILED");
    // As the dead process left it: two chunks committed, still RUNNING, the delivery never finished.
    world.dispatcher.abort();
    sqlx::query("update import_jobs set status = 'RUNNING', finished_at = null where id = $1")
        .bind(id.as_uuid())
        .execute(world.pool())
        .await
        .unwrap();
    sqlx::query("update event_publication set completion_date = null, status = 'PUBLISHED'")
        .execute(world.pool())
        .await
        .unwrap();
    world.flaky.fail_on.store(0, Ordering::SeqCst);

    let restarted = Outbox::new(world.pool().clone(), world.clock.clone());
    restarted.subscribe(world.imports.listeners()).unwrap();
    let dispatcher = restarted.start();
    let done = world.finished(id).await;

    assert_eq!(done["status"], "COMPLETED");
    assert_eq!(done["importedRows"], 450);
    assert_eq!(world.expenses().await, 450);
    dispatcher.abort();
}

/// A `RUNNING` import whose job runs in this process is not started a second time.
#[tokio::test]
async fn a_running_job_is_not_started_twice_in_one_process() {
    let world = World::new().await;
    let rows: Vec<String> = (0..300).map(good_row).collect();
    let id = world.upload(&csv(&rows)).await.id();
    // Delivered again while it may still run (the minute's re-delivery): either it is live and left alone,
    // or it is finished and left alone.
    world.imports.runner.run(world.team, id).await.unwrap();
    world.finished(id).await;

    assert_eq!(world.expenses().await, 300);
}

/// An amount over `numeric(19,4)` is that row's error (the reviewer's decision after M8); in Java the insert
/// fails and the import with it.
#[tokio::test]
async fn an_amount_too_large_for_the_database_is_a_row_error() {
    let world = World::new().await;
    let mut rows: Vec<String> = (0..150).map(good_row).collect();
    rows[120] = "2026-01-05,Cafe,12345678901234567.00,EUR,MEALS,".to_owned();

    let done = world.finished(world.upload(&csv(&rows)).await.id()).await;

    assert_eq!(done["status"], "COMPLETED");
    assert_eq!(done["importedRows"], 149);
    assert_eq!(done["errors"][0]["line"], 122);
    assert_eq!(done["errors"][0]["code"], "AMOUNT_INVALID");
}

/// Java: `ImportApiTest.theLimitIsPerUserAndPerTeamAndResetsTomorrow`, and the `Retry-After` until UTC midnight.
#[tokio::test]
async fn the_daily_limit_counts_the_utc_day_and_retry_after_runs_to_midnight() {
    let world = World::new().await;
    world.clock.set(Utc.with_ymd_and_hms(2026, 11, 16, 23, 59, 30).unwrap());
    for _ in 0..5 {
        assert_eq!(world.upload(&csv(&[good_row(1)])).await.status, StatusCode::ACCEPTED);
    }

    let sixth = world.upload(&csv(&[good_row(1)])).await;

    assert_eq!(sixth.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(sixth.code(), "RATE_LIMITED");
    assert_eq!(sixth.body["detail"], "At most 5 imports per team per day");
    assert_eq!(sixth.headers["retry-after"], "30");
    // Another team of the same user has its own allowance.
    let other = TeamId::new_v7();
    world.add_team(other).await;
    world.member(other, world.importer, "MEMBER").await;
    assert_eq!(
        world
            .upload_to(other, world.importer, csv(&[good_row(1)]).as_bytes())
            .await
            .status,
        StatusCode::ACCEPTED
    );
    // A minute later it is the next UTC day.
    world.clock.advance(TimeDelta::seconds(60));
    assert_eq!(world.upload(&csv(&[good_row(1)])).await.status, StatusCode::ACCEPTED);
}

/// Not a teammate, and the daily limit, are decided before a byte of the body is read.
#[tokio::test]
async fn whoever_may_not_import_is_refused_before_the_body_is_read() {
    let world = World::new().await;
    let garbage = |team: TeamId| {
        Request::post(format!("/api/v1/teams/{team}/imports"))
            .header("content-type", "multipart/form-data; boundary=b0undary")
            .body(HttpBody::from("not multipart"))
            .unwrap()
    };

    assert_eq!(
        world.send(garbage(world.team), UserId::new_v7()).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        world.send(garbage(world.team), world.importer).await.code(),
        "MALFORMED_REQUEST"
    );
}

#[tokio::test]
async fn the_upload_checks_answer_with_javas_messages() {
    let world = World::new().await;
    let cases: [(&[u8], StatusCode, &str); 4] = [
        (b"", StatusCode::BAD_REQUEST, "The file is empty"),
        (
            b"a,b\n1,2\n",
            StatusCode::BAD_REQUEST,
            "The first line must be: date,merchant,amount,currency,category[,note]",
        ),
        (
            b"MZ\x90\0\x03\0\0\0\x04\0\0\0\xff\xff\0\0",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "The file must be UTF-8 text",
        ),
        (
            b"date,merchant,amount,currency,category\nx\0y\n",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "The file is not a CSV text file",
        ),
    ];
    for (content, status, message) in cases {
        let reply = world.upload_to(world.team, world.importer, content).await;
        assert_eq!(reply.status, status, "{message}");
        let detail = if status == StatusCode::BAD_REQUEST {
            reply.body["errors"][0]["message"].clone()
        } else {
            reply.body["detail"].clone()
        };
        assert_eq!(detail, message);
    }
    // Refused uploads use up nothing of the allowance.
    let jobs: i64 = sqlx::query_scalar("select count(*) from import_jobs")
        .fetch_one(world.pool())
        .await
        .unwrap();
    assert_eq!(jobs, 0);
}

#[tokio::test]
async fn an_import_is_visible_to_its_starter_and_owners_only() {
    let world = World::new().await;
    let id = world.upload(&csv(&[good_row(1)])).await.id();

    assert_eq!(world.get(id, world.importer).await.status, StatusCode::OK);
    assert_eq!(world.get(id, world.owner).await.status, StatusCode::OK);
    assert_eq!(world.get(id, world.approver).await.status, StatusCode::NOT_FOUND);
    assert_eq!(world.get(id, UserId::new_v7()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        world.get(ImportId::new_v4(), world.owner).await.status,
        StatusCode::NOT_FOUND
    );
}
