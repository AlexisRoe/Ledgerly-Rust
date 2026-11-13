//! The expense endpoints in-process (axum `oneshot`) against real Postgres: every status code the spec lists for
//! these paths, Java's order of rejections, and the M3 abuse tests of Java's plan.
//!
//! Java: `ExpensesApiTest`. Callers are put into the request as [`CurrentUser`], as the authentication layer
//! will do from M5 on; memberships and receipts come from test doubles.

use std::sync::Arc;

use axum::Router;
use axum::body::Body as HttpBody;
use axum::http::{Request, StatusCode};
use chrono::{TimeZone, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::Role;
use ledgerly_shared::web::{CurrentUser, render};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::ExpensesApi;
use crate::application::MembershipLookup;
use crate::application::doubles::{FixedReceipts, Memberships};
use crate::{ReceiptChecks, ReceiptSummary};

/// A team of four, and the routes wired to it. Every expense "has" one accepted receipt.
struct World {
    _database: TestDatabase,
    router: Router,
    team: TeamId,
    member: UserId,
    colleague: UserId,
    approver: UserId,
    owner: UserId,
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

    fn etag(&self) -> &str {
        self.headers["etag"].to_str().unwrap()
    }
}

impl World {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let team = TeamId::new_v7();
        let (member, colleague, approver, owner) =
            (UserId::new_v7(), UserId::new_v7(), UserId::new_v7(), UserId::new_v7());
        let memberships: Arc<dyn MembershipLookup> = Arc::new(
            Memberships::default()
                .with(team, member, Role::Member)
                .with(team, colleague, Role::Member)
                .with(team, approver, Role::Approver)
                .with(team, owner, Role::Owner),
        );
        let receipts: Arc<dyn ReceiptChecks> = Arc::new(FixedReceipts(ReceiptSummary::new(1, 0)));
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, 0).unwrap()));
        let routes = ExpensesApi::new(database.pool(), &clock, &memberships, &receipts, 100).routes();
        let router = Router::new()
            .fallback_service(routes.into_service())
            .layer(axum::middleware::from_fn(render));
        Self {
            _database: database,
            router,
            team,
            member,
            colleague,
            approver,
            owner,
        }
    }

    fn expenses(&self) -> String {
        format!("/api/v1/teams/{}/expenses", self.team)
    }

    async fn send(&self, caller: Option<UserId>, request: axum::http::request::Builder, body: Option<Value>) -> Reply {
        let mut request = match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(HttpBody::from(body.to_string())),
            None => request.body(HttpBody::empty()),
        }
        .unwrap();
        if let Some(caller) = caller {
            request.extensions_mut().insert(CurrentUser::new(caller));
        }
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 16).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply { status, headers, body }
    }

    async fn create_as(&self, caller: UserId, body: Value) -> Reply {
        self.send(Some(caller), Request::post(self.expenses()), Some(body))
            .await
    }

    /// A draft of the member's; returns its path.
    async fn draft(&self) -> String {
        let reply = self.create_as(self.member, lunch()).await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        format!("{}/{}", self.expenses(), reply.body["id"].as_str().unwrap())
    }

    async fn get(&self, caller: UserId, path: &str) -> Reply {
        self.send(Some(caller), Request::get(path), None).await
    }

    async fn patch(&self, caller: UserId, path: &str, if_match: Option<&str>, body: Value) -> Reply {
        let mut request = Request::patch(path);
        if let Some(if_match) = if_match {
            request = request.header("if-match", if_match);
        }
        self.send(Some(caller), request, Some(body)).await
    }

    async fn delete(&self, caller: UserId, path: &str, if_match: Option<&str>) -> Reply {
        let mut request = Request::delete(path);
        if let Some(if_match) = if_match {
            request = request.header("if-match", if_match);
        }
        self.send(Some(caller), request, None).await
    }

    async fn act(&self, caller: UserId, path: &str, action: &str) -> Reply {
        let body = (action == "reject").then(|| json!({"reason": "blurry"}));
        self.send(Some(caller), Request::post(format!("{path}:{action}")), body)
            .await
    }
}

fn lunch() -> Value {
    json!({
        "money": {"amount": "12.5", "currency": "EUR"},
        "category": "MEALS",
        "incurredOn": "2026-11-05",
        "merchant": "Cafe Central",
        "note": "Team lunch",
    })
}

fn with(mut body: Value, key: &str, value: Value) -> Value {
    body[key] = value;
    body
}

// --- create -----------------------------------------------------------------------------------------------------

/// Java: `ExpensesApiTest.createReturns201WithLocationAndEtag`.
#[tokio::test]
async fn create_is_201_with_location_etag_and_the_callers_draft() {
    let world = World::new().await;

    let reply = world.create_as(world.member, lunch()).await;

    assert_eq!(reply.status, StatusCode::CREATED);
    let id = reply.body["id"].as_str().unwrap();
    assert_eq!(reply.headers["location"], format!("{}/{id}", world.expenses()));
    assert_eq!(reply.etag(), "\"0\"");
    assert_eq!(reply.headers["content-type"], "application/json");
    assert_eq!(
        reply.body,
        json!({
            "id": id,
            "teamId": world.team.to_string(),
            "claimantId": world.member.to_string(),
            "money": {"amount": "12.50", "currency": "EUR"},
            "category": "MEALS",
            "incurredOn": "2026-11-05",
            "merchant": "Cafe Central",
            "note": "Team lunch",
            "status": "DRAFT",
            "rejectionReasons": [],
        })
    );
}

#[tokio::test]
async fn create_refuses_bad_input_with_javas_codes_and_fields() {
    let world = World::new().await;
    let cases = [
        // Java: `ExpensesApiTest.tooManyDecimalsIs400` (the scale rule comes from `Money`, field `money`).
        (
            with(lunch(), "money", json!({"amount": "12.345", "currency": "EUR"})),
            "VALIDATION_FAILED",
            "money",
        ),
        (
            with(lunch(), "money", json!({"amount": "12.50", "currency": "ZZZ"})),
            "VALIDATION_FAILED",
            "money",
        ),
        (
            with(lunch(), "money", json!({"amount": "0", "currency": "EUR"})),
            "VALIDATION_FAILED",
            "money",
        ),
        // `@Pattern` on the generated model, field path as Spring names nested fields.
        (
            with(lunch(), "money", json!({"amount": "abc", "currency": "EUR"})),
            "VALIDATION_FAILED",
            "money.amount",
        ),
        (
            with(lunch(), "money", json!({"amount": "1", "currency": "eur"})),
            "VALIDATION_FAILED",
            "money.currency",
        ),
        // Java: `ExpensesApiTest.futureDateIs400`, from the domain.
        (
            with(lunch(), "incurredOn", json!("2026-11-07")),
            "VALIDATION_FAILED",
            "incurredOn",
        ),
        (with(lunch(), "merchant", json!("")), "VALIDATION_FAILED", "merchant"),
        (with(lunch(), "merchant", json!("   ")), "VALIDATION_FAILED", "merchant"),
        (
            with(lunch(), "note", json!("x".repeat(2001))),
            "VALIDATION_FAILED",
            "note",
        ),
    ];
    for (body, code, field) in cases {
        let reply = world.create_as(world.member, body.clone()).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(
            (reply.code(), reply.body["errors"][0]["field"].as_str()),
            (code, Some(field)),
            "{body}"
        );
    }

    // Java: `ExpensesApiTest.missingFieldsAre400WithEveryField`.
    let reply = world.create_as(world.member, json!({"note": "x"})).await;
    assert_eq!(reply.code(), "VALIDATION_FAILED");
    assert_eq!(
        reply.body["errors"],
        json!([
            {"field": "money", "message": "must not be null"},
            {"field": "category", "message": "must not be null"},
            {"field": "incurredOn", "message": "must not be null"},
            {"field": "merchant", "message": "must not be null"},
        ])
    );

    // Unknown properties (mass assignment, OWASP API3), wrong types and broken JSON: `MALFORMED_REQUEST`.
    for body in [
        with(lunch(), "status", json!("APPROVED")),
        with(lunch(), "claimantId", json!(world.owner.to_string())),
        with(lunch(), "category", json!("BOGUS")),
        with(lunch(), "incurredOn", json!("yesterday")),
        with(lunch(), "money", json!({"amount": "1", "currency": "EUR", "rate": 2})),
    ] {
        let reply = world.create_as(world.member, body.clone()).await;
        assert_eq!(
            (reply.status, reply.code()),
            (StatusCode::BAD_REQUEST, "MALFORMED_REQUEST"),
            "{body}"
        );
    }
    let broken = world
        .send(
            Some(world.member),
            Request::post(world.expenses()).header("content-type", "application/json"),
            None,
        )
        .await;
    assert_eq!(broken.code(), "MALFORMED_REQUEST");
}

#[tokio::test]
async fn create_checks_the_idempotency_key_length_after_the_body() {
    let world = World::new().await;
    let request = |key: &str| Request::post(world.expenses()).header("idempotency-key", key);

    let short = world.send(Some(world.member), request("short"), Some(lunch())).await;
    assert_eq!(short.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        short.body["errors"],
        json!([{"field": "idempotencyKey", "message": "size must be between 8 and 128"}])
    );
    let both = world.send(Some(world.member), request("short"), Some(json!({}))).await;
    assert_eq!(both.body["errors"][0]["field"], "money", "the body is checked first");
}

#[tokio::test]
async fn create_is_401_without_a_caller_and_404_for_a_non_teammate() {
    let world = World::new().await;

    let anonymous = world.send(None, Request::post(world.expenses()), Some(lunch())).await;
    assert_eq!(
        (anonymous.status, anonymous.code()),
        (StatusCode::UNAUTHORIZED, "UNAUTHENTICATED")
    );
    assert_eq!(anonymous.headers["www-authenticate"], "Bearer");

    let stranger = world.create_as(UserId::new_v7(), lunch()).await;
    assert_eq!((stranger.status, stranger.code()), (StatusCode::NOT_FOUND, "NOT_FOUND"));
    let other_team = world
        .send(
            Some(world.member),
            Request::post(format!("/api/v1/teams/{}/expenses", TeamId::new_v7())),
            Some(lunch()),
        )
        .await;
    assert_eq!(other_team.status, StatusCode::NOT_FOUND);
}

// --- get --------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn get_is_200_with_the_create_etag_and_404_for_whoever_may_not_see_it() {
    let world = World::new().await;
    let path = world.draft().await;

    let own = world.get(world.member, &path).await;
    assert_eq!((own.status, own.etag()), (StatusCode::OK, "\"0\""));
    assert_eq!(own.body["status"], "DRAFT");
    assert_eq!(world.get(world.approver, &path).await.status, StatusCode::OK);

    for caller in [world.colleague, UserId::new_v7()] {
        let hidden = world.get(caller, &path).await;
        assert_eq!((hidden.status, hidden.code()), (StatusCode::NOT_FOUND, "NOT_FOUND"));
        assert_eq!(hidden.body["detail"], "Resource not found");
        assert_eq!(hidden.body["instance"], path);
    }
    let anonymous = world.send(None, Request::get(&path), None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    let not_a_uuid = world.get(world.member, &format!("{}/abc", world.expenses())).await;
    assert_eq!(
        (not_a_uuid.status, not_a_uuid.code()),
        (StatusCode::BAD_REQUEST, "REQUEST_REJECTED")
    );
}

// --- patch ------------------------------------------------------------------------------------------------------

/// Java: `ExpensesApiTest.patchChangesFieldsAndBumpsEtag` and the precondition tests.
#[tokio::test]
async fn patch_needs_a_current_if_match() {
    let world = World::new().await;
    let path = world.draft().await;
    let change = json!({"merchant": "Bakery", "note": "rolls"});

    let missing = world.patch(world.member, &path, None, change.clone()).await;
    assert_eq!(
        (missing.status, missing.code()),
        (StatusCode::PRECONDITION_REQUIRED, "PRECONDITION_REQUIRED")
    );
    assert_eq!(missing.body["detail"], "The If-Match header is required");
    for stale in ["\"99\"", "garbage"] {
        let reply = world.patch(world.member, &path, Some(stale), change.clone()).await;
        assert_eq!(
            (reply.status, reply.code()),
            (StatusCode::PRECONDITION_FAILED, "PRECONDITION_FAILED"),
            "{stale}"
        );
    }
    let too_long = world
        .patch(world.member, &path, Some(&"1".repeat(65)), change.clone())
        .await;
    assert_eq!(too_long.body["errors"][0]["field"], "ifMatch");

    let updated = world.patch(world.member, &path, Some("W/\"0\""), change.clone()).await;
    assert_eq!((updated.status, updated.etag()), (StatusCode::OK, "\"1\""));
    assert_eq!(
        (updated.body["merchant"].as_str(), updated.body["note"].as_str()),
        (Some("Bakery"), Some("rolls"))
    );

    let again = world.patch(world.member, &path, Some("\"0\""), change).await;
    assert_eq!(
        again.status,
        StatusCode::PRECONDITION_FAILED,
        "the old ETag is stale now"
    );
}

#[tokio::test]
async fn patch_cannot_set_the_status_or_the_claimant() {
    let world = World::new().await;
    let path = world.draft().await;

    for body in [
        json!({"status": "APPROVED"}),
        json!({"claimantId": world.owner.to_string()}),
    ] {
        let reply = world.patch(world.member, &path, Some("\"0\""), body.clone()).await;
        assert_eq!(
            (reply.status, reply.code()),
            (StatusCode::BAD_REQUEST, "MALFORMED_REQUEST"),
            "{body}"
        );
    }
    assert_eq!(world.get(world.member, &path).await.etag(), "\"0\"");
}

#[tokio::test]
async fn patch_refuses_in_javas_order() {
    let world = World::new().await;
    let path = world.draft().await;
    let rename = json!({"merchant": "Other"});

    let colleague = world.patch(world.colleague, &path, Some("\"0\""), rename.clone()).await;
    assert_eq!(colleague.status, StatusCode::NOT_FOUND);
    let approver = world.patch(world.approver, &path, Some("\"0\""), rename.clone()).await;
    assert_eq!((approver.status, approver.code()), (StatusCode::FORBIDDEN, "FORBIDDEN"));
    let future = world
        .patch(world.member, &path, Some("\"0\""), json!({"incurredOn": "2026-12-01"}))
        .await;
    assert_eq!(future.body["errors"][0]["field"], "incurredOn");
    let text = world
        .send(
            Some(world.member),
            Request::patch(&path)
                .header("if-match", "\"0\"")
                .header("content-type", "text/plain"),
            None,
        )
        .await;
    assert_eq!(text.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    world.act(world.member, &path, "submit").await;
    let submitted = world.patch(world.member, &path, Some("\"1\""), rename).await;
    assert_eq!(
        (submitted.status, submitted.code()),
        (StatusCode::CONFLICT, "EXPENSE_NOT_DRAFT")
    );
}

#[tokio::test]
async fn a_patch_that_changes_nothing_keeps_the_etag() {
    let world = World::new().await;
    let path = world.draft().await;

    let same = world
        .patch(world.member, &path, Some("\"0\""), json!({"merchant": "Cafe Central"}))
        .await;
    let empty = world.patch(world.member, &path, Some("\"0\""), json!({})).await;

    assert_eq!((same.status, same.etag()), (StatusCode::OK, "\"0\""));
    assert_eq!(empty.etag(), "\"0\"");
}

#[tokio::test]
async fn a_blank_note_in_a_patch_clears_it() {
    let world = World::new().await;
    let path = world.draft().await;

    let reply = world
        .patch(world.member, &path, Some("\"0\""), json!({"note": ""}))
        .await;

    assert_eq!(reply.body["note"], Value::Null);
}

// --- delete -----------------------------------------------------------------------------------------------------

/// Java: `ExpensesApiTest.deleteThenGet404`.
#[tokio::test]
async fn delete_is_204_and_then_the_expense_is_gone() {
    let world = World::new().await;
    let path = world.draft().await;

    assert_eq!(
        world.delete(world.member, &path, None).await.status,
        StatusCode::PRECONDITION_REQUIRED
    );
    assert_eq!(
        world.delete(world.member, &path, Some("\"5\"")).await.status,
        StatusCode::PRECONDITION_FAILED
    );
    let approver = world.delete(world.approver, &path, Some("\"0\"")).await;
    assert_eq!(approver.status, StatusCode::FORBIDDEN);
    assert_eq!(
        world.delete(world.colleague, &path, Some("\"0\"")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        world.send(None, Request::delete(&path), None).await.status,
        StatusCode::UNAUTHORIZED
    );

    let deleted = world.delete(world.member, &path, Some("\"0\"")).await;
    assert_eq!((deleted.status, &deleted.body), (StatusCode::NO_CONTENT, &Value::Null));
    assert_eq!(world.get(world.member, &path).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_submitted_expense_cannot_be_deleted() {
    let world = World::new().await;
    let path = world.draft().await;
    world.act(world.member, &path, "submit").await;

    let reply = world.delete(world.member, &path, Some("\"1\"")).await;

    assert_eq!(
        (reply.status, reply.code()),
        (StatusCode::CONFLICT, "EXPENSE_NOT_DRAFT")
    );
}

// --- transitions ------------------------------------------------------------------------------------------------

/// Java: `ExpensesApiTest.fullWorkflowApproveAndReimburse` and `rejectReopenSubmitWithdraw`.
#[tokio::test]
async fn the_six_transitions_answer_200_with_the_new_etag() {
    let world = World::new().await;
    let path = world.draft().await;

    let steps = [
        (world.member, "submit", "SUBMITTED"),
        (world.approver, "reject", "REJECTED"),
        (world.member, "reopen", "DRAFT"),
        (world.member, "submit", "SUBMITTED"),
        (world.member, "withdraw", "DRAFT"),
        (world.member, "submit", "SUBMITTED"),
        (world.approver, "approve", "APPROVED"),
        (world.owner, "reimburse", "REIMBURSED"),
    ];
    for (number, (caller, action, status)) in (1..).zip(steps) {
        let reply = world.act(caller, &path, action).await;
        assert_eq!(reply.status, StatusCode::OK, "{action}: {}", reply.body);
        assert_eq!(reply.body["status"], status);
        assert_eq!(reply.etag(), format!("\"{number}\""));
    }
    assert_eq!(
        world.get(world.owner, &path).await.body["rejectionReasons"],
        json!(["blurry"])
    );
}

/// Java's M3 abuse tests: a member's `:approve` is 403, self-approval 409, approving a draft 409, submitting
/// without a receipt 422, an empty reason 400, a colleague 404.
#[tokio::test]
async fn transitions_are_refused_with_javas_codes() {
    let world = World::new().await;
    let path = world.draft().await;

    let member = world.act(world.member, &path, "approve").await;
    assert_eq!((member.status, member.code()), (StatusCode::FORBIDDEN, "FORBIDDEN"));
    let draft = world.act(world.approver, &path, "approve").await;
    assert_eq!(
        (draft.status, draft.code()),
        (StatusCode::CONFLICT, "EXPENSE_INVALID_TRANSITION")
    );
    assert_eq!(draft.body["type"], "https://ledgerly.dev/problems/invalid-transition");
    let colleague = world.act(world.colleague, &path, "submit").await;
    assert_eq!(colleague.status, StatusCode::NOT_FOUND);
    let anonymous = world.send(None, Request::post(format!("{path}:submit")), None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    let empty_reason = world
        .send(
            Some(world.approver),
            Request::post(format!("{path}:reject")),
            Some(json!({"reason": ""})),
        )
        .await;
    assert_eq!(
        (empty_reason.status, empty_reason.code()),
        (StatusCode::BAD_REQUEST, "VALIDATION_FAILED")
    );
    assert_eq!(empty_reason.body["errors"][0]["field"], "reason");

    let own = world.create_as(world.approver, lunch()).await;
    let own = format!("{}/{}", world.expenses(), own.body["id"].as_str().unwrap());
    world.act(world.approver, &own, "submit").await;
    let self_approval = world.act(world.approver, &own, "approve").await;
    assert_eq!(
        (self_approval.status, self_approval.code()),
        (StatusCode::CONFLICT, "EXPENSE_SELF_DEALING")
    );
}

#[tokio::test]
async fn submit_without_an_accepted_receipt_is_422() {
    let database = TestDatabase::new().await.unwrap();
    let team = TeamId::new_v7();
    let member = UserId::new_v7();
    let memberships: Arc<dyn MembershipLookup> = Arc::new(Memberships::default().with(team, member, Role::Member));
    let receipts: Arc<dyn ReceiptChecks> = Arc::new(FixedReceipts(ReceiptSummary::new(0, 0)));
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, 0).unwrap()));
    let router = Router::new()
        .fallback_service(
            ExpensesApi::new(database.pool(), &clock, &memberships, &receipts, 100)
                .routes()
                .into_service(),
        )
        .layer(axum::middleware::from_fn(render));
    let call = |request: Request<HttpBody>| {
        let mut request = request;
        request.extensions_mut().insert(CurrentUser::new(member));
        router.clone().oneshot(request)
    };
    let created = call(
        Request::post(format!("/api/v1/teams/{team}/expenses"))
            .header("content-type", "application/json")
            .body(HttpBody::from(lunch().to_string()))
            .unwrap(),
    )
    .await
    .unwrap();
    let created: Value =
        serde_json::from_slice(&axum::body::to_bytes(created.into_body(), 4096).await.unwrap()).unwrap();

    let response = call(
        Request::post(format!(
            "/api/v1/teams/{team}/expenses/{}:submit",
            created["id"].as_str().unwrap()
        ))
        .body(HttpBody::empty())
        .unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["code"], "EXPENSE_RULE_VIOLATED");
    assert_eq!(body["detail"], "At least one accepted receipt is required to submit");
}

#[tokio::test]
async fn an_unknown_action_and_a_wrong_method_are_springs_rejections() {
    let world = World::new().await;
    let path = world.draft().await;

    let unknown = world.act(world.member, &path, "explode").await;
    assert_eq!(
        (unknown.status, unknown.code()),
        (StatusCode::METHOD_NOT_ALLOWED, "REQUEST_REJECTED")
    );
    let get_action = world.get(world.member, &format!("{path}:approve")).await;
    assert_eq!(
        (get_action.status, get_action.code()),
        (StatusCode::BAD_REQUEST, "REQUEST_REJECTED")
    );
}

// --- idempotency (M3.5) -----------------------------------------------------------------------------------------

/// Java: `ExpensesApiTest.sameKeySameBodyReplaysTheOriginal`, `sameKeyDifferentBodyIs422`,
/// `keysArePerUser` and `withoutAKeyEveryPostCreates`.
#[tokio::test]
async fn an_idempotency_key_replays_the_same_request_and_refuses_another() {
    let world = World::new().await;
    let keyed = |caller: UserId, body: Value| {
        world.send(
            Some(caller),
            Request::post(world.expenses()).header("idempotency-key", "key-12345678"),
            Some(body),
        )
    };

    let first = keyed(world.member, lunch()).await;
    let replay = keyed(world.member, lunch()).await;
    assert_eq!(
        (first.status, replay.status),
        (StatusCode::CREATED, StatusCode::CREATED)
    );
    assert_eq!(replay.body["id"], first.body["id"]);
    assert_eq!(replay.headers["location"], first.headers["location"]);

    let different = keyed(world.member, with(lunch(), "merchant", json!("Other"))).await;
    assert_eq!(
        (different.status, different.code()),
        (StatusCode::UNPROCESSABLE_ENTITY, "IDEMPOTENCY_KEY_REUSED")
    );
    assert_eq!(
        different.body["detail"],
        "This Idempotency-Key was used with a different request"
    );

    let colleague = keyed(world.colleague, lunch()).await;
    assert_eq!(colleague.status, StatusCode::CREATED);
    assert_ne!(colleague.body["id"], first.body["id"]);

    let unkeyed = world.create_as(world.member, lunch()).await;
    let again = world.create_as(world.member, lunch()).await;
    assert_ne!(unkeyed.body["id"], again.body["id"]);
}
