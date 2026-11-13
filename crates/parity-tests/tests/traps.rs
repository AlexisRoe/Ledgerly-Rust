//! Behaviour the OpenAPI file does not show but Java's tests or Bruno assert: the parity traps of
//! `docs/reference/java-test-inventory.md` §4.
//!
//! A trap that a ported Java test already asserts is not repeated here (the error codes of the expense
//! endpoints in `expenses.rs`, the cookie attributes in `auth.rs`, the CORS rules in
//! `owasp_api8_security_misconfiguration.rs`, …). What is here has no Java test of its own, or only a Bruno
//! request, or is spread over several Java tests that each assert a part. Traps that cannot be seen from
//! outside are in `NOT_PARITY.md`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::time::Duration;

use ledgerly_parity_tests::http::Response;
use ledgerly_parity_tests::{V1, World, eventually, expense_json, today};
use serde_json::{Value, json};
use uuid::Uuid;

/// `ErrorCatalogue`: code → (status, slug of `type`, title). Port spec §3.3.
const CATALOGUE: &[(&str, u16, &str, &str)] = &[
    (
        "VALIDATION_FAILED",
        400,
        "validation-failed",
        "Request validation failed",
    ),
    ("MALFORMED_REQUEST", 400, "malformed-request", "Malformed request"),
    ("UNAUTHENTICATED", 401, "unauthenticated", "Authentication required"),
    ("FORBIDDEN", 403, "forbidden", "Action not allowed for your role"),
    ("NOT_FOUND", 404, "not-found", "Resource not found"),
    (
        "EXPENSE_INVALID_TRANSITION",
        409,
        "invalid-transition",
        "Illegal workflow transition",
    ),
    (
        "EXPENSE_SELF_DEALING",
        409,
        "self-dealing",
        "Self-dealing is not allowed",
    ),
    ("LAST_OWNER", 409, "last-owner", "A team must keep at least one owner"),
    ("ALREADY_MEMBER", 409, "already-member", "Already a teammate"),
    (
        "INVITATION_NOT_PENDING",
        409,
        "invitation-not-pending",
        "Invitation is no longer pending",
    ),
    (
        "INVITATION_EXPIRED",
        409,
        "invitation-expired",
        "Invitation has expired",
    ),
    (
        "EMAIL_NOT_VERIFIED",
        403,
        "email-not-verified",
        "Verify your email address first",
    ),
    ("EXPENSE_NOT_DRAFT", 409, "not-draft", "Only a draft allows this"),
    (
        "IDEMPOTENCY_IN_PROGRESS",
        409,
        "idempotency-in-progress",
        "Request already in progress",
    ),
    ("PRECONDITION_FAILED", 412, "precondition-failed", "ETag does not match"),
    (
        "PRECONDITION_REQUIRED",
        428,
        "precondition-required",
        "If-Match header required",
    ),
    ("EXPENSE_RULE_VIOLATED", 422, "rule-violated", "Business rule violated"),
    (
        "IDEMPOTENCY_KEY_REUSED",
        422,
        "idempotency-key-reused",
        "Idempotency key reused with a different body",
    ),
    ("PAYLOAD_TOO_LARGE", 413, "payload-too-large", "Request body too large"),
    (
        "UNSUPPORTED_MEDIA_TYPE",
        415,
        "unsupported-media-type",
        "Unsupported file type",
    ),
    (
        "RECEIPT_DUPLICATE",
        409,
        "receipt-duplicate",
        "This file is already attached to the expense",
    ),
    ("RATE_LIMITED", 429, "rate-limited", "Too many requests"),
    (
        "BATCH_REJECTED",
        422,
        "batch-rejected",
        "The batch was rejected as a whole",
    ),
    ("INTERNAL_ERROR", 500, "internal-error", "Unexpected error"),
    ("REQUEST_REJECTED", 400, "request-rejected", "Request rejected"),
];

/// Checks a problem body against the catalogue and that it carries a W3C trace id (§4.1 "When the problem
/// body appears"). `REQUEST_REJECTED` keeps the status Spring chose, so only its other members are checked.
fn assert_catalogued(response: &Response, code: &str) {
    let body = response.json();
    let (_, status, slug, title) = CATALOGUE
        .iter()
        .find(|(known, ..)| *known == code)
        .unwrap_or_else(|| panic!("{code} is not in the catalogue"));
    assert_eq!(body["code"], code, "{body}");
    if code != "REQUEST_REJECTED" {
        assert_eq!(response.status, *status, "{body}");
    }
    assert_eq!(body["status"], response.status, "{body}");
    assert_eq!(body["type"], format!("https://ledgerly.dev/problems/{slug}"), "{body}");
    assert_eq!(body["title"], *title, "{body}");
    assert_eq!(response.media_type().as_deref(), Some("application/problem+json"));
    let trace_id = body["traceId"]
        .as_str()
        .unwrap_or_else(|| panic!("no traceId in {body}"));
    assert!(
        trace_id.len() == 32 && trace_id.bytes().all(|b| b.is_ascii_hexdigit()),
        "{trace_id}"
    );
}

fn id(response: &Response) -> String {
    response.json()["id"].as_str().unwrap().to_owned()
}

mod m3 {
    use super::*;

    /// §4.1: an unknown route is a problem with `REQUEST_REJECTED`, its catalogue type and a trace id.
    #[tokio::test]
    #[ignore = "M3: expects the catalogue's type and title for REQUEST_REJECTED, but Java's ProblemFactory.enrich (src/main/java/com/ledgerly/shared/error/ProblemFactory.java) only adds code and traceId to Spring's own problem, which keeps type about:blank and title Not Found; raised in learning log M3"]
    async fn an_unknown_route_is_a_catalogued_problem_with_a_trace_id() {
        let world = World::start().await;

        let response = world.anonymous().get(&format!("{V1}/nope")).send_unchecked().await;

        assert_eq!(response.status, 404);
        assert_catalogued(&response, "REQUEST_REJECTED");
    }
}

mod m5 {
    use super::*;

    /// §4.1, Bruno `05-security/an over-long password is refused before any hashing`: a 200-character login
    /// password is `VALIDATION_FAILED`, not a failed login.
    #[tokio::test]
    #[ignore = "M5"]
    async fn an_over_long_login_password_is_a_validation_error() {
        let world = World::start().await;

        let response = world.login("alice@example.com", &"x".repeat(200)).await;

        assert_eq!(response.status, 400);
        assert_catalogued(&response, "VALIDATION_FAILED");
    }

    /// §4.1: the problem bodies of the authentication layer are catalogued like every other.
    #[tokio::test]
    #[ignore = "M5"]
    async fn a_401_is_a_catalogued_problem_with_a_trace_id() {
        let world = World::start().await;

        let response = world.anonymous().get(&format!("{V1}/me")).send().await;

        assert_catalogued(&response, "UNAUTHENTICATED");
        assert_eq!(response.json()["instance"], format!("{V1}/me"));
    }

    /// §4.2 "Authentication before routing": without a token any non-public path is 401, even one that does
    /// not exist; with a token the same path is 404.
    #[tokio::test]
    #[ignore = "M5"]
    async fn authentication_comes_before_routing() {
        let world = World::start().await;
        let user = world.user().await;

        for path in [format!("{V1}/does-not-exist"), "/whatever".to_owned()] {
            assert_eq!(
                world.anonymous().get(&path).send_unchecked().await.status,
                401,
                "{path}"
            );
            assert_eq!(
                world.as_user(&user).get(&path).send_unchecked().await.status,
                404,
                "{path}"
            );
        }
    }

    /// §4.4 "Lockout": failures are counted per email whether or not the account exists (Java's learning log,
    /// M10), so an address that is registered after five failures starts locked.
    #[tokio::test]
    #[ignore = "M5"]
    async fn failures_count_per_email_even_before_the_account_exists() {
        let world = World::start().await;
        let email = format!("later-{}@example.com", Uuid::new_v4());
        for _ in 0..5 {
            assert_eq!(world.login(&email, "wrong-password-123").await.status, 401);
        }

        assert_eq!(world.register(&email, "S3cret-Passw0rd-xyz").await.status, 202);

        assert_eq!(world.login(&email, "S3cret-Passw0rd-xyz").await.status, 401);
    }

    /// §4.4 "Auth": emails are stored lower-case, and logging in is case-insensitive in the address.
    #[tokio::test]
    #[ignore = "M5"]
    async fn emails_are_stored_lower_case() {
        let world = World::start().await;
        let email = format!("Mixed.Case-{}@Example.COM", Uuid::new_v4().simple());

        assert_eq!(world.register(&email, "S3cret-Passw0rd-xyz").await.status, 202);

        let stored: String = sqlx::query_scalar("select email from users where lower(email) = lower($1)")
            .bind(&email)
            .fetch_one(world.fixtures.pool())
            .await
            .unwrap();
        assert_eq!(stored, email.to_lowercase());
        assert_eq!(
            world.login(&email.to_uppercase(), "S3cret-Passw0rd-xyz").await.status,
            200
        );
    }
}

mod m6 {
    use super::*;

    /// §4.1: the codes a teammate can provoke with one request each carry their catalogue type and title.
    #[tokio::test]
    #[ignore = "M6"]
    async fn problems_carry_their_catalogue_type_and_title() {
        let world = World::start().await;
        let team = world.team().await;
        let member = world.as_user(&team.member);
        let expenses = format!("{}/expenses", team.path());
        let created = world
            .create_expense(&team, &team.member, &expense_json("10.00", "Cafe"))
            .await;
        let path = team.expense_path(id(&created).parse().unwrap());

        assert_catalogued(
            &member.post(&expenses).json(&json!({})).send().await,
            "VALIDATION_FAILED",
        );
        assert_catalogued(
            &member
                .post(&expenses)
                .body("application/json", "{not json")
                .send()
                .await,
            "MALFORMED_REQUEST",
        );
        assert_catalogued(
            &member.get(&team.expense_path(Uuid::new_v4())).send().await,
            "NOT_FOUND",
        );
        assert_catalogued(&member.post(&format!("{path}:approve")).send().await, "FORBIDDEN");
        assert_catalogued(
            &world
                .as_user(&team.approver)
                .post(&format!("{path}:approve"))
                .send()
                .await,
            "EXPENSE_INVALID_TRANSITION",
        );
        assert_catalogued(
            &member.patch(&path).json(&json!({"merchant": "A"})).send().await,
            "PRECONDITION_REQUIRED",
        );
        assert_catalogued(
            &member
                .patch(&path)
                .header("If-Match", "\"99\"")
                .json(&json!({"merchant": "A"}))
                .send()
                .await,
            "PRECONDITION_FAILED",
        );
        let only_owner = world.team().await;
        assert_catalogued(
            &world
                .as_user(&only_owner.owner)
                .post(&format!("{}:leave", only_owner.path()))
                .send()
                .await,
            "LAST_OWNER",
        );
    }

    /// §4.4 "Money and text fields": the number of decimals is the currency's, not the spec's 4.
    #[tokio::test]
    #[ignore = "M6"]
    async fn the_scale_of_an_amount_is_the_currencys() {
        let world = World::start().await;
        let team = world.team().await;
        let (world, team) = (&world, &team);
        let create = |amount: &str, currency: &str| {
            let body = json!({
                "money": {"amount": amount, "currency": currency},
                "category": "MEALS",
                "incurredOn": today(),
                "merchant": "Cafe",
            });
            async move { world.create_expense(team, &team.member, &body).await }
        };

        assert_eq!(create("1.5", "JPY").await.status, 400);
        assert_eq!(create("10.001", "EUR").await.status, 400);
        let kwd = create("1.001", "KWD").await;
        assert_eq!(kwd.status, 201);
        assert_eq!(kwd.json()["money"]["amount"], "1.001");
    }

    /// §4.4: zero and negative amounts are refused, and the pseudo-currencies too.
    #[tokio::test]
    #[ignore = "M6"]
    async fn zero_negative_and_non_monetary_amounts_are_refused() {
        let world = World::start().await;
        let team = world.team().await;

        for (amount, currency) in [
            ("0", "EUR"),
            ("0.00", "EUR"),
            ("-1.00", "EUR"),
            ("1.00", "XAU"),
            ("1.00", "XTS"),
            ("1.00", "XXX"),
        ] {
            let body = json!({
                "money": {"amount": amount, "currency": currency},
                "category": "MEALS",
                "incurredOn": today(),
                "merchant": "Cafe",
            });
            let response = world.create_expense(&team, &team.member, &body).await;
            assert_eq!(response.status, 400, "{amount} {currency}");
            assert_eq!(response.json()["errors"][0]["field"], "money", "{amount} {currency}");
        }
    }

    /// §4.4: responses normalise the amount to the currency's scale.
    #[tokio::test]
    #[ignore = "M6"]
    async fn amounts_are_normalised_to_the_currencys_scale() {
        let world = World::start().await;
        let team = world.team().await;

        let created = world
            .create_expense(&team, &team.member, &expense_json("12.5", "Cafe"))
            .await;

        assert_eq!(created.json()["money"]["amount"], "12.50");
    }

    /// §4.4: today is allowed (the server's UTC date).
    #[tokio::test]
    #[ignore = "M6"]
    async fn an_expense_incurred_today_is_allowed() {
        let world = World::start().await;
        let team = world.team().await;

        let created = world
            .create_expense(&team, &team.member, &expense_json("1.00", "Cafe"))
            .await;

        assert_eq!(created.status, 201);
        assert_eq!(created.json()["incurredOn"], today());
    }

    /// §4.4 "IDs": expense ids are UUIDv7.
    #[tokio::test]
    #[ignore = "M6"]
    async fn expense_ids_are_uuid_v7() {
        let world = World::start().await;
        let team = world.team().await;

        let created = world
            .create_expense(&team, &team.member, &expense_json("1.00", "Cafe"))
            .await;

        assert_eq!(id(&created).parse::<Uuid>().unwrap().get_version_num(), 7);
    }

    /// §4.4 "Idempotency": a replay returns 201 with the same id and the expense as it is now (Java's learning
    /// log, M3).
    #[tokio::test]
    #[ignore = "M6"]
    async fn an_idempotent_replay_returns_the_current_state() {
        let world = World::start().await;
        let team = world.team().await;
        let member = world.as_user(&team.member);
        let key = format!("key-{}", Uuid::new_v4().simple());
        let create = || {
            member
                .post(&format!("{}/expenses", team.path()))
                .header("Idempotency-Key", &key)
                .json(&expense_json("10.00", "Cafe"))
                .send()
        };
        let first = create().await;
        member
            .patch(&team.expense_path(id(&first).parse().unwrap()))
            .header("If-Match", first.header("etag").unwrap())
            .json(&json!({"merchant": "Bakery"}))
            .send()
            .await;

        let replay = create().await;

        assert_eq!(replay.status, 201);
        assert_eq!(id(&replay), id(&first));
        assert_eq!(replay.json()["merchant"], "Bakery");
    }
}

mod m7 {
    use super::*;

    /// §4.2, Bruno `03-lifecycle` seq 4: a MEMBER approving their own DRAFT gets 403, not the 409 of the state
    /// or of self-dealing.
    #[tokio::test]
    #[ignore = "M7"]
    async fn a_member_approving_their_own_draft_gets_403() {
        let world = World::start().await;
        let team = world.team().await;
        let draft = world.expense(&team, &team.member, "DRAFT").await;

        let response = world
            .as_user(&team.member)
            .post(&format!("{}:approve", team.expense_path(draft)))
            .send()
            .await;

        assert_eq!(response.status, 403);
    }

    /// §4.2 "If-Match", Bruno `03-lifecycle` seq 6: every transition bumps the version, a receipt upload does
    /// not. After create ("0") and submit the ETag is "1".
    #[tokio::test]
    #[ignore = "M7"]
    async fn transitions_bump_the_version_and_receipt_uploads_do_not() {
        let world = World::start().await;
        let team = world.team().await;
        let member = world.as_user(&team.member);
        let created = world
            .create_expense(&team, &team.member, &expense_json("10.00", "Cafe"))
            .await;
        let expense: Uuid = id(&created).parse().unwrap();
        world
            .fixtures
            .accepted_receipt(team.id, expense, &team.member.user)
            .await;
        assert_eq!(
            member.get(&team.expense_path(expense)).send().await.header("etag"),
            Some("\"0\"")
        );

        let submitted = member
            .post(&format!("{}:submit", team.expense_path(expense)))
            .send()
            .await;

        assert_eq!(submitted.status, 200);
        assert_eq!(
            member.get(&team.expense_path(expense)).send().await.header("etag"),
            Some("\"1\"")
        );
        let edit = member
            .patch(&team.expense_path(expense))
            .header("If-Match", "\"1\"")
            .json(&json!({"merchant": "A"}))
            .send()
            .await;
        assert_eq!(edit.problem_code().as_deref(), Some("EXPENSE_NOT_DRAFT"));
    }
}

mod m9 {
    use super::*;

    async fn trail(world: &World, owner: &ledgerly_parity_tests::Session, team: Uuid, query: &str) -> Value {
        world
            .as_user(owner)
            .get(&format!("{V1}/teams/{team}/audit-events{query}"))
            .send()
            .await
            .json()
    }

    /// §4.5: `MEMBER_INVITED` records the role and nothing about the address.
    #[tokio::test]
    #[ignore = "M9"]
    async fn member_invited_records_the_role_only() {
        let world = World::start().await;
        let team = world.team().await;
        let email = format!("invitee-{}@example.com", Uuid::new_v4());
        world
            .as_user(&team.owner)
            .post(&format!("{}/invitations", team.path()))
            .json(&json!({"email": email, "role": "APPROVER"}))
            .send()
            .await;

        let event = eventually("MEMBER_INVITED in the trail", Duration::from_secs(30), || async {
            let body = trail(&world, &team.owner, team.id, "").await;
            body["items"]
                .as_array()?
                .iter()
                .find(|item| item["eventType"] == "MEMBER_INVITED")
                .cloned()
        })
        .await;

        assert_eq!(event["details"], json!({"role": "APPROVER"}));
        assert!(!event.to_string().contains(&email));
    }

    /// §4.5: the scanner's verdict has no actor.
    #[tokio::test]
    #[ignore = "M9"]
    async fn receipt_accepted_has_no_actor() {
        let world = World::start().await;
        let team = world.team().await;
        let expense = world.expense(&team, &team.member, "DRAFT").await;
        world
            .as_user(&team.member)
            .post(&format!("{}/receipts", team.expense_path(expense)))
            .file(
                "file",
                "r.pdf",
                "application/pdf",
                ledgerly_parity_tests::fixtures::PDF.to_vec(),
            )
            .send()
            .await;

        let event = eventually("RECEIPT_ACCEPTED in the trail", Duration::from_secs(30), || async {
            let body = trail(&world, &team.owner, team.id, "").await;
            body["items"]
                .as_array()?
                .iter()
                .find(|item| item["eventType"] == "RECEIPT_ACCEPTED")
                .cloned()
        })
        .await;

        assert!(event.get("actorId").is_none_or(Value::is_null), "{event}");
    }

    /// §4.5: without `limit` a page has 50 rows.
    #[tokio::test]
    #[ignore = "M9"]
    async fn the_default_limit_is_50() {
        let world = World::start().await;
        let team = world.team().await;
        for n in 0..60 {
            sqlx::query(
                "insert into audit_events (id, event_id, team_id, event_type, details, occurred_at, created_at) \
                 values ($1, $2, $3, 'SEEDED', jsonb_build_object('n', $4::int), now(), now())",
            )
            .bind(Uuid::now_v7())
            .bind(Uuid::new_v4())
            .bind(team.id)
            .bind(n)
            .execute(world.fixtures.pool())
            .await
            .unwrap();
        }

        let body = trail(&world, &team.owner, team.id, "").await;

        assert_eq!(body["items"].as_array().unwrap().len(), 50);
        assert!(body["nextCursor"].is_string());
    }
}
