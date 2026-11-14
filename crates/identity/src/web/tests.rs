//! The identity endpoints in-process (axum `oneshot`) against real Postgres, with a clock the tests move.
//!
//! Java: `AuthApiTest`, including the cases that need a clock and so are not in the black-box parity suite:
//! the verification token expiring after 24 hours and the login lock ending after 15 minutes.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body as HttpBody;
use axum::http::{HeaderMap, Request, StatusCode};
use chrono::{TimeDelta, TimeZone, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::config::{AuthConfig, Profiles};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::events::{Event, Publisher};
use ledgerly_shared::ratelimit::RateLimiter;
use ledgerly_shared::web::render;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::Identity;
use crate::events::IdentityEvent;
use crate::opaque_token::TokenHash;

const PASSWORD: &str = "S3cret-Passw0rd-xyz";

/// Keeps the type and the actor of every event, in order. It sees an event when the use case publishes it,
/// before its transaction commits.
#[derive(Debug, Default)]
struct Recorded(Mutex<Vec<(&'static str, String)>>);

impl Publisher for Recorded {
    fn publish<'a>(
        &'a self,
        _connection: &'a mut sqlx::PgConnection,
        event: &'a dyn Event,
    ) -> Pin<Box<dyn Future<Output = sqlx::Result<()>> + Send + 'a>> {
        let actor = format!("{event:?}");
        self.0.lock().unwrap().push((event.event_type(), actor));
        Box::pin(std::future::ready(Ok(())))
    }
}

struct World {
    database: TestDatabase,
    clock: Arc<FixedClock>,
    router: Router,
    identity: Identity,
    events: Arc<Recorded>,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or_default()
    }

    fn cookie(&self) -> String {
        let header = self.headers["set-cookie"].to_str().unwrap();
        header
            .strip_prefix("refresh_token=")
            .and_then(|rest| rest.split_once(';'))
            .map(|(value, _)| value.to_owned())
            .unwrap()
    }
}

fn auth() -> AuthConfig {
    AuthConfig {
        access_token_ttl: Duration::from_secs(600),
        refresh_token_ttl: Duration::from_secs(14 * 86_400),
        verification_ttl: Duration::from_secs(86_400),
        max_failed_logins: 5,
        lock_duration: Duration::from_secs(15 * 60),
        jwt_private_key: None,
        jwt_public_key: None,
    }
}

impl World {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let clock = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 13, 12, 0, 0).unwrap()));
        let shared: Arc<dyn Clock> = clock.clone();
        let limiter = RateLimiter::new(shared.clone());
        let profiles = Profiles {
            test: true,
            ..Profiles::default()
        };
        let events = Arc::new(Recorded::default());
        let publisher: Arc<dyn Publisher> = events.clone();
        let identity = Identity::new(&auth(), profiles, database.pool(), &shared, &publisher, &limiter).unwrap();
        let router = Router::new()
            .fallback_service(identity.routes().into_service())
            .layer(axum::middleware::from_fn(render));
        Self {
            database,
            clock,
            router,
            identity,
            events,
        }
    }

    fn pool(&self) -> &PgPool {
        self.database.pool()
    }

    async fn post(&self, path: &str, body: &Value) -> Reply {
        let request = Request::post(path)
            .header("content-type", "application/json")
            .body(HttpBody::from(body.to_string()))
            .unwrap();
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

    /// `POST path` without a body, with optional `X-Requested-With` values and a `Cookie` header.
    async fn cookie_post(&self, path: &str, requested_with: &[&str], cookie: Option<&str>) -> Reply {
        let mut request = Request::post(path);
        for value in requested_with {
            request = request.header("x-requested-with", *value);
        }
        if let Some(cookie) = cookie {
            request = request.header("cookie", format!("refresh_token={cookie}"));
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(HttpBody::empty()).unwrap())
            .await
            .unwrap();
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

    async fn refresh(&self, cookie: &str) -> Reply {
        self.cookie_post("/api/v1/auth/refresh", &["ledgerly"], Some(cookie))
            .await
    }

    async fn logout(&self, cookie: &str) -> Reply {
        self.cookie_post("/api/v1/auth/logout", &["ledgerly"], Some(cookie))
            .await
    }

    /// A registered user's refresh cookie after logging in.
    async fn session(&self) -> String {
        let email = self.registered().await;
        let reply = self.login(&email, PASSWORD).await;
        assert_eq!(reply.status, StatusCode::OK);
        reply.cookie()
    }

    async fn register(&self, email: &str, password: &str) -> Reply {
        self.post("/api/v1/auth/register", &json!({"email": email, "password": password}))
            .await
    }

    async fn login(&self, email: &str, password: &str) -> Reply {
        self.post("/api/v1/auth/login", &json!({"email": email, "password": password}))
            .await
    }

    async fn verify(&self, token: &str) -> Reply {
        self.post("/api/v1/auth/verify", &json!({"token": token})).await
    }

    /// Registers a fresh address and returns it.
    async fn registered(&self) -> String {
        let email = format!("user-{}@example.com", Uuid::new_v4());
        assert_eq!(self.register(&email, PASSWORD).await.status, StatusCode::ACCEPTED);
        email
    }

    async fn user_id(&self, email: &str) -> Uuid {
        sqlx::query_scalar("select id from users where email = $1")
            .bind(email)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    /// A second verification of the user's with a token the test knows (the issued one is only in the log).
    async fn verification_token(&self, email: &str, expires_in: TimeDelta) -> String {
        let token = format!("known-token-{}", Uuid::new_v4().simple());
        sqlx::query("insert into email_verifications (id, user_id, token_hash, expires_at) values ($1, $2, $3, $4)")
            .bind(Uuid::now_v7())
            .bind(self.user_id(email).await)
            .bind(TokenHash::of(&token.clone().into()).as_str().to_owned())
            .bind(self.clock.now() + expires_in)
            .execute(self.pool())
            .await
            .unwrap();
        token
    }

    async fn verified(&self, email: &str) -> bool {
        sqlx::query_scalar("select verified from users where email = $1")
            .bind(email)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }
}

// --- register ---

/// Java: `AuthApiTest.registerAnswersIdenticallyForNewAndTakenEmails`.
#[tokio::test]
async fn register_answers_identically_for_new_and_taken_emails() {
    let world = World::new().await;
    let email = format!("user-{}@example.com", Uuid::new_v4());

    let first = world.register(&email, PASSWORD).await;
    let again = world.register(&email, PASSWORD).await;
    let upper = world.register(&email.to_uppercase(), "Another-Passw0rd").await;

    for reply in [&first, &again, &upper] {
        assert_eq!(reply.status, StatusCode::ACCEPTED);
        assert_eq!(
            reply.body,
            json!({"message": "If this address can be registered, a verification message has been sent."})
        );
    }
    let rows: i64 = sqlx::query_scalar("select count(*) from users where lower(email) = lower($1)")
        .bind(&email)
        .fetch_one(world.pool())
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn a_new_account_is_unverified_lower_case_and_has_one_verification_for_24_hours() {
    let world = World::new().await;
    let trimmed = format!("Mixed.Case-{}@Example.COM", Uuid::new_v4().simple());

    assert_eq!(world.register(&trimmed, PASSWORD).await.status, StatusCode::ACCEPTED);

    let (id, stored, hash, verified, created_at): (Uuid, String, String, bool, chrono::DateTime<Utc>) =
        sqlx::query_as("select id, email, password_hash, verified, created_at from users where email = $1")
            .bind(trimmed.to_lowercase())
            .fetch_one(world.pool())
            .await
            .unwrap();
    assert_eq!(stored, trimmed.to_lowercase());
    assert_eq!(id.get_version_num(), 7);
    assert!(hash.starts_with("$argon2id$v=19$m=16384,t=2,p=1$"), "{hash}");
    assert!(!verified);
    assert_eq!(created_at, world.clock.now());
    let (token_hash, expires_at): (String, chrono::DateTime<Utc>) =
        sqlx::query_as("select token_hash, expires_at from email_verifications where user_id = $1")
            .bind(id)
            .fetch_one(world.pool())
            .await
            .unwrap();
    assert_eq!(token_hash.len(), 64);
    assert_eq!(expires_at, world.clock.now() + TimeDelta::hours(24));
}

/// Java: `AuthApiTest.weakOrOversizedInputIsRejected`, with Bean Validation's messages.
#[tokio::test]
async fn weak_malformed_or_oversized_input_is_a_validation_problem() {
    let world = World::new().await;
    let email = format!("user-{}@example.com", Uuid::new_v4());
    let long_email = format!("{}@example.com", "a".repeat(250));

    for (body, field, message) in [
        (
            json!({"email": email, "password": "short"}),
            "password",
            "size must be between 12 and 128",
        ),
        (
            json!({"email": "not-an-email", "password": PASSWORD}),
            "email",
            "must be a well-formed email address",
        ),
        (
            json!({"email": email, "password": "x".repeat(129)}),
            "password",
            "size must be between 12 and 128",
        ),
        (
            json!({"email": long_email, "password": PASSWORD}),
            "email",
            "size must be between 0 and 254",
        ),
        (json!({"password": PASSWORD}), "email", "must not be null"),
        // Hibernate's @Email accepts "", Java registers it; refused here (decision after M5).
        (
            json!({"email": "", "password": PASSWORD}),
            "email",
            "must be a well-formed email address",
        ),
        (json!({"email": email}), "password", "must not be null"),
    ] {
        let reply = world.post("/api/v1/auth/register", &body).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(reply.code(), "VALIDATION_FAILED", "{body}");
        assert_eq!(
            reply.body["errors"][0],
            json!({"field": field, "message": message}),
            "{body}"
        );
    }
    let users: i64 = sqlx::query_scalar("select count(*) from users")
        .fetch_one(world.pool())
        .await
        .unwrap();
    assert_eq!(users, 0);
}

#[tokio::test]
async fn unknown_members_or_a_non_json_body_are_refused() {
    let world = World::new().await;

    let extra = world
        .post(
            "/api/v1/auth/register",
            &json!({"email": "a@example.com", "password": PASSWORD, "verified": true}),
        )
        .await;

    assert_eq!(extra.code(), "MALFORMED_REQUEST");
}

/// Passwords are counted in UTF-16 units, as Java's `String.length()`: 12 emoji are 24 units.
#[tokio::test]
async fn password_length_is_counted_like_javas() {
    let world = World::new().await;

    let reply = world
        .register(&format!("u-{}@example.com", Uuid::new_v4()), &"😀".repeat(65))
        .await;

    assert_eq!(reply.body["errors"][0]["message"], "size must be between 12 and 128");
    assert_eq!(
        world
            .register(&format!("u-{}@example.com", Uuid::new_v4()), &"😀".repeat(6))
            .await
            .status,
        StatusCode::ACCEPTED
    );
}

// --- verify ---

/// Java: `AuthApiTest.verificationTokenWorksOnceAndMarksTheUserVerified`.
#[tokio::test]
async fn a_verification_token_works_once_and_marks_the_user_verified() {
    let world = World::new().await;
    let email = world.registered().await;
    let token = world.verification_token(&email, TimeDelta::hours(24)).await;

    let first = world.verify(&token).await;
    let second = world.verify(&token).await;

    assert_eq!(first.status, StatusCode::NO_CONTENT);
    assert!(world.verified(&email).await);
    assert_eq!(second.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        second.body["errors"],
        json!([{"field": "token", "message": "Invalid or expired token"}])
    );
}

/// Java: `AuthApiTest.verificationTokenExpiresAfter24Hours`.
#[tokio::test]
async fn a_verification_token_expires_after_24_hours() {
    let world = World::new().await;
    let email = world.registered().await;
    let token = world.verification_token(&email, TimeDelta::hours(24)).await;

    world.clock.advance(TimeDelta::hours(25));

    assert_eq!(world.verify(&token).await.status, StatusCode::BAD_REQUEST);
    assert!(!world.verified(&email).await);
}

#[tokio::test]
async fn a_verification_token_is_usable_until_the_instant_it_expires() {
    let world = World::new().await;
    let email = world.registered().await;
    let token = world.verification_token(&email, TimeDelta::hours(24)).await;

    world.clock.advance(TimeDelta::hours(24));

    assert_eq!(world.verify(&token).await.status, StatusCode::BAD_REQUEST);
    world.clock.advance(-TimeDelta::milliseconds(1));
    assert_eq!(world.verify(&token).await.status, StatusCode::NO_CONTENT);
}

/// Java: `AuthApiTest.guessedVerificationTokensAreRejected`, and a token too short to be one.
#[tokio::test]
async fn guessed_or_malformed_verification_tokens_are_refused() {
    let world = World::new().await;

    assert_eq!(world.verify(&"a".repeat(43)).await.code(), "VALIDATION_FAILED");
    let short = world.verify("abc").await;
    assert_eq!(short.body["errors"][0]["message"], "size must be between 20 and 200");
}

// --- login ---

/// Java: `AuthApiTest.loginReturnsAccessTokenAndHardenedRefreshCookie` and `unverifiedUsersMayLogIn`.
#[tokio::test]
async fn login_returns_an_access_token_and_a_hardened_refresh_cookie() {
    let world = World::new().await;
    let email = world.registered().await;

    let reply = world.login(&email, PASSWORD).await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["tokenType"], "Bearer");
    assert_eq!(reply.body["expiresIn"], 600);
    let names: Vec<&str> = reply.body.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(names, ["accessToken", "expiresIn", "tokenType"]);
    let token = reply.body["accessToken"].as_str().unwrap();
    let user = world.user_id(&email).await;
    let tokens = world.identity.access_tokens().unwrap();
    assert_eq!(tokens.verify(token).map(|id| id.as_uuid()), Ok(user));
    let set_cookie = reply.headers["set-cookie"].to_str().unwrap();
    let start = format!(
        "refresh_token={}; Path=/api/v1/auth; Max-Age=1209600; Expires=",
        reply.cookie()
    );
    assert!(set_cookie.starts_with(&start), "{set_cookie}");
    assert!(
        set_cookie.ends_with("; Secure; HttpOnly; SameSite=Strict"),
        "{set_cookie}"
    );
    assert_eq!(reply.headers["cache-control"], "no-store");
    assert!(!reply.body.to_string().contains(&reply.cookie()));
}

#[tokio::test]
async fn login_stores_only_the_hash_of_a_new_refresh_family_for_14_days() {
    let world = World::new().await;
    let email = world.registered().await;

    let reply = world.login(&email, PASSWORD).await;

    let (id, family, token_hash, expires_at): (Uuid, Uuid, String, chrono::DateTime<Utc>) =
        sqlx::query_as("select id, family_id, token_hash, expires_at from refresh_tokens where user_id = $1")
            .bind(world.user_id(&email).await)
            .fetch_one(world.pool())
            .await
            .unwrap();
    assert_eq!((id.get_version_num(), family.get_version_num()), (7, 4));
    assert_eq!(token_hash, TokenHash::of(&reply.cookie().into()).as_str());
    assert_eq!(expires_at, world.clock.now() + TimeDelta::days(14));
}

/// Java: `AuthApiTest.unknownEmailAndWrongPasswordLookTheSame`: no challenge header and no cookie either.
#[tokio::test]
async fn unknown_email_and_wrong_password_look_the_same() {
    let world = World::new().await;
    let email = world.registered().await;

    let wrong = world.login(&email, "wrong-password-123").await;
    let unknown = world
        .login(&format!("nobody-{}@example.com", Uuid::new_v4()), "wrong-password-123")
        .await;

    for reply in [&wrong, &unknown] {
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.code(), "UNAUTHENTICATED");
        assert_eq!(reply.body["detail"], "Authentication required");
        assert!(reply.headers.get("set-cookie").is_none());
        assert!(reply.headers.get("www-authenticate").is_none());
    }
    assert_eq!(wrong.body, unknown.body);
}

/// Java: `AuthApiTest.accountLocksAfterFiveFailuresEvenForTheRightPassword`, with its clock step.
#[tokio::test]
async fn the_address_locks_after_five_failures_until_the_period_is_over() {
    let world = World::new().await;
    let email = world.registered().await;
    for _ in 0..5 {
        assert_eq!(
            world.login(&email, "wrong-password-123").await.status,
            StatusCode::UNAUTHORIZED
        );
    }

    let locked = world.login(&email, PASSWORD).await;
    assert_eq!(locked.status, StatusCode::UNAUTHORIZED);
    assert!(!locked.body.to_string().to_lowercase().contains("lock"));

    world.clock.advance(TimeDelta::minutes(14));
    assert_eq!(world.login(&email, PASSWORD).await.status, StatusCode::UNAUTHORIZED);

    world.clock.advance(TimeDelta::minutes(2));
    assert_eq!(world.login(&email, PASSWORD).await.status, StatusCode::OK);
}

/// Java: `AuthApiTest.aSuccessfulLoginResetsTheFailureCounter`.
#[tokio::test]
async fn a_successful_login_resets_the_failure_counter() {
    let world = World::new().await;
    let email = world.registered().await;

    for _ in 0..4 {
        world.login(&email, "wrong-password-123").await;
    }
    assert_eq!(world.login(&email, PASSWORD).await.status, StatusCode::OK);
    for _ in 0..4 {
        world.login(&email, "wrong-password-123").await;
    }
    assert_eq!(world.login(&email, PASSWORD).await.status, StatusCode::OK);
}

/// Inventory §4.4: failures are counted per address whether or not an account exists.
#[tokio::test]
async fn failures_count_per_email_even_before_the_account_exists() {
    let world = World::new().await;
    let email = format!("later-{}@example.com", Uuid::new_v4());
    for _ in 0..5 {
        world.login(&email, "wrong-password-123").await;
    }

    assert_eq!(world.register(&email, PASSWORD).await.status, StatusCode::ACCEPTED);

    assert_eq!(world.login(&email, PASSWORD).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_lock_is_per_normalised_address() {
    let world = World::new().await;
    let email = world.registered().await;
    for _ in 0..5 {
        world.login(&email.to_uppercase(), "wrong-password-123").await;
    }

    assert_eq!(world.login(&email, PASSWORD).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        world.login(&world.registered().await, PASSWORD).await.status,
        StatusCode::OK
    );
}

/// An account managed by an identity provider has no password: logging in to it with one is a wrong password.
#[tokio::test]
async fn an_account_without_a_password_cannot_log_in_with_one() {
    let world = World::new().await;
    let email = format!("external-{}@example.com", Uuid::new_v4());
    sqlx::query(
        "insert into users (id, email, password_hash, verified, created_at, external_subject) \
         values ($1, $2, null, true, now(), $3)",
    )
    .bind(Uuid::now_v7())
    .bind(&email)
    .bind(Uuid::new_v4().to_string())
    .execute(world.pool())
    .await
    .unwrap();

    assert_eq!(world.login(&email, PASSWORD).await.status, StatusCode::UNAUTHORIZED);
}

/// Bruno `05-security`: a 200-character password is refused by validation, before any hashing.
#[tokio::test]
async fn an_over_long_login_password_is_a_validation_error() {
    let world = World::new().await;

    let reply = world.login("alice@example.com", &"x".repeat(200)).await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        reply.body["errors"],
        json!([{"field": "password", "message": "size must be between 1 and 128"}])
    );
}

// --- refresh and logout ---

/// Java: `AuthApiTest.refreshRotatesTheTokenAndIssuesANewAccessToken`.
#[tokio::test]
async fn refresh_rotates_the_token_and_issues_a_new_access_token() {
    let world = World::new().await;
    let first = world.session().await;

    let refreshed = world.refresh(&first).await;

    assert_eq!(refreshed.status, StatusCode::OK);
    assert_ne!(refreshed.cookie(), first);
    assert_eq!(refreshed.headers["cache-control"], "no-store");
    let tokens = world.identity.access_tokens().unwrap();
    assert!(tokens.verify(refreshed.body["accessToken"].as_str().unwrap()).is_ok());
    assert_eq!(world.refresh(&refreshed.cookie()).await.status, StatusCode::OK);
}

/// Java: `AuthApiTest.replayingARotatedTokenRevokesTheWholeFamily`: also the successor dies.
#[tokio::test]
async fn replaying_a_rotated_token_revokes_the_whole_family() {
    let world = World::new().await;
    let first = world.session().await;
    let second = world.refresh(&first).await.cookie();

    let replay = world.refresh(&first).await;

    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);
    assert!(replay.headers.get("set-cookie").is_none());
    assert_eq!(world.refresh(&second).await.status, StatusCode::UNAUTHORIZED);
    let live: i64 = sqlx::query_scalar("select count(*) from refresh_tokens where revoked_at is null")
        .fetch_one(world.pool())
        .await
        .unwrap();
    assert_eq!(live, 0);
}

/// Plan M5.6: two refreshes with one token at the same moment give exactly one success (the row lock makes the
/// second see a replaced token).
#[tokio::test]
async fn concurrent_refreshes_with_one_token_give_exactly_one_success() {
    let world = World::new().await;
    let token = world.session().await;

    let (one, two) = tokio::join!(world.refresh(&token), world.refresh(&token));

    let mut statuses = [one.status, two.status];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::UNAUTHORIZED]);
}

#[tokio::test]
async fn a_session_slides_14_days_from_each_refresh() {
    let world = World::new().await;
    let mut token = world.session().await;

    // Five refreshes 13 days apart keep the session alive for 65 days.
    for _ in 0..5 {
        world.clock.advance(TimeDelta::days(13));
        let reply = world.refresh(&token).await;
        assert_eq!(reply.status, StatusCode::OK);
        token = reply.cookie();
    }

    world.clock.advance(TimeDelta::days(14));
    assert_eq!(world.refresh(&token).await.status, StatusCode::UNAUTHORIZED);
}

/// Java: `AuthApiTest.refreshNeedsTheCustomHeaderAndTheCookie`, with Java's messages and checks in Spring's
/// order.
#[tokio::test]
async fn refresh_needs_the_custom_header_and_the_cookie() {
    let world = World::new().await;
    let token = world.session().await;
    let path = "/api/v1/auth/refresh";

    let missing = world.cookie_post(path, &[], Some(&token)).await;
    let wrong = world.cookie_post(path, &["XMLHttpRequest"], Some(&token)).await;
    let twice = world.cookie_post(path, &["ledgerly", "ledgerly"], Some(&token)).await;
    let long = world.cookie_post(path, &["wrong"], Some(&"a".repeat(201))).await;
    let no_cookie = world.cookie_post(path, &["ledgerly"], None).await;
    let blank = world.cookie_post(path, &["ledgerly"], Some("")).await;

    assert_eq!(
        missing.body["errors"],
        json!([{"field": "X-Requested-With", "message": "header is required"}])
    );
    assert_eq!(
        wrong.body["errors"],
        json!([{"field": "X-Requested-With", "message": "must be 'ledgerly'"}])
    );
    assert_eq!(twice.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        long.body["errors"],
        json!([{"field": "refreshToken", "message": "size must be between 0 and 200"}])
    );
    assert_eq!(
        (no_cookie.status, blank.status),
        (StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED)
    );
    assert!(no_cookie.headers.get("www-authenticate").is_none());
    // None of the refused attempts used the token up.
    assert_eq!(world.refresh(&token).await.status, StatusCode::OK);
}

/// Java: `AuthApiTest.unknownRefreshTokenGets401`.
#[tokio::test]
async fn an_unknown_refresh_token_is_401() {
    let world = World::new().await;

    assert_eq!(world.refresh(&"a".repeat(43)).await.code(), "UNAUTHENTICATED");
}

/// Java: `AuthApiTest.logoutRevokesTheFamilyAndClearsTheCookie`.
#[tokio::test]
async fn logout_revokes_the_family_and_clears_the_cookie() {
    let world = World::new().await;
    let first = world.session().await;
    let second = world.refresh(&first).await.cookie();

    let out = world.logout(&second).await;

    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert_eq!(
        out.headers["set-cookie"],
        "refresh_token=; Path=/api/v1/auth; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Secure; HttpOnly; \
         SameSite=Strict"
    );
    assert_eq!(world.refresh(&second).await.status, StatusCode::UNAUTHORIZED);
}

/// Java: `AuthApiTest.logoutIsIdempotentButStillNeedsTheCustomHeader`.
#[tokio::test]
async fn logout_is_idempotent_but_still_needs_the_custom_header() {
    let world = World::new().await;

    assert_eq!(
        world
            .cookie_post("/api/v1/auth/logout", &["ledgerly"], None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(world.logout(&"a".repeat(43)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(
        world.cookie_post("/api/v1/auth/logout", &[], None).await.status,
        StatusCode::BAD_REQUEST
    );
}

// --- events ---

impl World {
    fn event_types(&self) -> Vec<&'static str> {
        self.events.0.lock().unwrap().iter().map(|(name, _)| *name).collect()
    }
}

/// Java publishes `UserRegistered`, `UserVerified` and `RefreshTokenReuseDetected`, each with ids only.
#[tokio::test]
async fn registering_verifying_and_a_replay_publish_their_events_with_ids_only() {
    let world = World::new().await;
    let email = world.registered().await;
    let user = world.user_id(&email).await;
    let token = world.verification_token(&email, TimeDelta::hours(24)).await;
    world.verify(&token).await;
    let first = world.login(&email, PASSWORD).await.cookie();
    world.refresh(&first).await;
    world.refresh(&first).await;
    // Neither a taken address nor a failed login publishes anything.
    world.register(&email, PASSWORD).await;
    world.login(&email, "wrong-password-123").await;

    assert_eq!(
        world.event_types(),
        [
            "ledgerly-rust:identity.UserRegistered",
            "ledgerly-rust:identity.UserVerified",
            "ledgerly-rust:identity.RefreshTokenReuseDetected",
        ]
    );
    for (_, printed) in world.events.0.lock().unwrap().iter() {
        assert!(printed.contains(&user.to_string()), "{printed}");
        assert!(!printed.contains(&email) && !printed.contains(&first), "{printed}");
    }
}

#[test]
fn an_event_has_a_v7_id_and_names_its_actor() {
    let actor = ledgerly_shared::id::UserId::new_v7();
    let event = IdentityEvent::user_registered(actor, Utc::now());

    assert_eq!(event.event_id().get_version_num(), 7);
    assert_eq!(event.actor_id(), actor);
}

// --- jwks ---

#[tokio::test]
async fn the_jwks_holds_only_the_public_key_named_by_its_kid() {
    let world = World::new().await;
    let request = Request::get("/.well-known/jwks.json").body(HttpBody::empty()).unwrap();

    let response = world.router.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 16).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let keys = body["keys"].as_array().unwrap();
    assert_eq!(keys.len(), 1);
    let names: Vec<&str> = keys[0].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(names, ["crv", "kid", "kty", "x", "y"]);
    assert_eq!(keys[0]["kid"], world.identity.access_tokens().unwrap().keys().kid());
}
