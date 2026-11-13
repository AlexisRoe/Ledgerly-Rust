//! Accounts and sessions: register, verify, log in, access tokens, refresh and log out.
//!
//! Java: `src/test/java/com/ledgerly/identity/internal/web/AuthApiTest.java`.
//!
//! Java reads verification tokens from a test mailbox and mints access tokens in-process. Here a verification
//! with a known token is written with SQL, and tokens come from logging in (ADR 0011).
//!
//! Not here, because they need a clock (see `NOT_PARITY.md`): the verification token expiring after 24 hours,
//! the lock ending after 15 minutes, the access token expiring after 10 minutes and the refresh token after
//! 14 days.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::fixtures::User;
use ledgerly_parity_tests::http::Response;
use ledgerly_parity_tests::{V1, World, jwt};
use serde_json::json;
use uuid::Uuid;

/// Java: `AuthApiTest.PASSWORD`.
const PASSWORD: &str = "S3cret-Passw0rd-xyz";

fn new_email() -> String {
    format!("user-{}@example.com", Uuid::new_v4())
}

/// Java: `registeredEmail()`.
async fn registered(world: &World) -> User {
    let email = new_email();
    assert_eq!(world.register(&email, PASSWORD).await.status, 202);
    User {
        id: world.user_id_of(&email).await,
        email,
    }
}

/// Java: `loggedIn()`: registered, verified and logged in.
async fn logged_in(world: &World) -> Response {
    let user = world.register_and_verify(PASSWORD).await;
    let response = world.login(&user.email, PASSWORD).await;
    assert_eq!(response.status, 200);
    response
}

fn access_token(response: &Response) -> String {
    response.json()["accessToken"].as_str().unwrap().to_owned()
}

fn cookie(response: &Response) -> String {
    response
        .cookie("refresh_token")
        .expect("the response sets the refresh cookie")
}

/// Java: `protectedStatusWith()`: the status of a team-scoped request with `bearer` as the token.
async fn protected_status_with(world: &World, bearer: &str) -> u16 {
    world
        .with_token(bearer)
        .get(&format!("{V1}/teams/{}/expenses", Uuid::new_v4()))
        .send()
        .await
        .status
}

mod m5 {
    use super::*;

    // --- registration (API2, API6) ---

    /// Java: `AuthApiTest.registerAnswersIdenticallyForNewAndTakenEmails`.
    #[tokio::test]
    async fn register_answers_identically_for_new_and_taken_emails() {
        let world = World::start().await;
        let email = new_email();

        let first = world.register(&email, PASSWORD).await;
        let second = world.register(&email, PASSWORD).await;
        let upper_case = world.register(&email.to_uppercase(), PASSWORD).await;

        assert_eq!((first.status, second.status, upper_case.status), (202, 202, 202));
        assert_eq!(second.json(), first.json());
        assert_eq!(upper_case.json(), first.json());
        let rows: i64 = sqlx::query_scalar("select count(*) from users where email = $1")
            .bind(&email)
            .fetch_one(world.fixtures.pool())
            .await
            .unwrap();
        assert_eq!(rows, 1);
    }

    /// Java: `AuthApiTest.passwordIsStoredAsAnArgon2idHash`.
    #[tokio::test]
    async fn password_is_stored_as_an_argon2id_hash() {
        let world = World::start().await;
        let user = registered(&world).await;

        let hash: String = sqlx::query_scalar("select password_hash from users where id = $1")
            .bind(user.id)
            .fetch_one(world.fixtures.pool())
            .await
            .unwrap();

        assert!(hash.starts_with("$argon2id$"), "{hash}");
        assert!(!hash.contains(PASSWORD));
    }

    /// Java: `AuthApiTest.weakOrOversizedInputIsRejected`.
    #[tokio::test]
    async fn weak_or_oversized_input_is_rejected() {
        let world = World::start().await;

        assert_eq!(world.register(&new_email(), "short").await.status, 400);
        assert_eq!(world.register("not-an-email", PASSWORD).await.status, 400);
        assert_eq!(world.register(&new_email(), &"x".repeat(129)).await.status, 400);
    }

    // --- verification ---

    /// Java: `AuthApiTest.verificationTokenWorksOnceAndMarksTheUserVerified`.
    #[tokio::test]
    async fn verification_token_works_once_and_marks_the_user_verified() {
        let world = World::start().await;
        let user = registered(&world).await;
        let token = world.fixtures.verification_token(&user).await;

        assert_eq!(world.verify(&token).await.status, 204);

        let verified: bool = sqlx::query_scalar("select verified from users where id = $1")
            .bind(user.id)
            .fetch_one(world.fixtures.pool())
            .await
            .unwrap();
        assert!(verified);
        assert_eq!(world.verify(&token).await.status, 400);
    }

    /// Java: `AuthApiTest.verificationTokenIsStoredOnlyAsAHash`. The token the application issued is not
    /// known here, but what it stored for it is: 64 hex characters, the size of a SHA-256.
    #[tokio::test]
    async fn verification_token_is_stored_only_as_a_hash() {
        let world = World::start().await;
        let user = registered(&world).await;

        let stored: String = sqlx::query_scalar("select token_hash from email_verifications where user_id = $1")
            .bind(user.id)
            .fetch_one(world.fixtures.pool())
            .await
            .unwrap();

        assert_eq!(stored.len(), 64, "{stored}");
        assert!(
            stored.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{stored}"
        );
    }

    /// Java: `AuthApiTest.guessedVerificationTokensAreRejected`.
    #[tokio::test]
    async fn guessed_verification_tokens_are_rejected() {
        let world = World::start().await;

        assert_eq!(world.verify(&"a".repeat(43)).await.status, 400);
    }

    // --- login (API2) ---

    /// Java: `AuthApiTest.loginReturnsAccessTokenAndHardenedRefreshCookie`.
    #[tokio::test]
    async fn login_returns_access_token_and_hardened_refresh_cookie() {
        let world = World::start().await;

        let response = logged_in(&world).await;

        let body = response.json();
        assert_eq!(body["tokenType"], "Bearer");
        assert_eq!(body["expiresIn"], 600);
        let set_cookie = response.header("set-cookie").unwrap();
        assert!(set_cookie.starts_with("refresh_token="), "{set_cookie}");
        for attribute in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/api/v1/auth"] {
            assert!(set_cookie.contains(attribute), "{attribute} is missing in {set_cookie}");
        }
        assert!(!response.text().contains(&cookie(&response)));
        assert_eq!(response.header("cache-control"), Some("no-store"));
    }

    /// Java: `AuthApiTest.unverifiedUsersMayLogIn`.
    #[tokio::test]
    async fn unverified_users_may_log_in() {
        let world = World::start().await;
        let user = registered(&world).await;

        assert_eq!(world.login(&user.email, PASSWORD).await.status, 200);
    }

    /// Java: `AuthApiTest.unknownEmailAndWrongPasswordLookTheSame`.
    #[tokio::test]
    async fn unknown_email_and_wrong_password_look_the_same() {
        let world = World::start().await;
        let user = registered(&world).await;

        let wrong_password = world.login(&user.email, "wrong-password-123").await;
        let unknown_email = world.login(&new_email(), "wrong-password-123").await;

        assert_eq!((wrong_password.status, unknown_email.status), (401, 401));
        for member in ["code", "detail", "title"] {
            assert_eq!(unknown_email.json()[member], wrong_password.json()[member], "{member}");
        }
        assert_eq!(wrong_password.header("set-cookie"), None);
    }

    /// Java: `AuthApiTest.accountLocksAfterFiveFailuresEvenForTheRightPassword`.
    #[tokio::test]
    async fn account_locks_after_five_failures_even_for_the_right_password() {
        let world = World::start().await;
        let user = registered(&world).await;
        for _ in 0..5 {
            assert_eq!(world.login(&user.email, "wrong-password-123").await.status, 401);
        }

        let locked = world.login(&user.email, PASSWORD).await;

        assert_eq!(locked.status, 401);
        let detail = locked.json()["detail"].as_str().unwrap_or_default().to_lowercase();
        assert!(!detail.contains("lock"), "{detail}");
    }

    /// Java: `AuthApiTest.aSuccessfulLoginResetsTheFailureCounter`.
    #[tokio::test]
    async fn a_successful_login_resets_the_failure_counter() {
        let world = World::start().await;
        let user = registered(&world).await;

        for _ in 0..4 {
            world.login(&user.email, "wrong-password-123").await;
        }
        assert_eq!(world.login(&user.email, PASSWORD).await.status, 200);
        for _ in 0..4 {
            world.login(&user.email, "wrong-password-123").await;
        }
        assert_eq!(world.login(&user.email, PASSWORD).await.status, 200);
    }

    // --- access tokens (API2) ---

    /// Java: `AuthApiTest.accessTokenCarriesOnlySubIatExpAndJti`.
    #[tokio::test]
    async fn access_token_carries_only_sub_iat_exp_and_jti() {
        let world = World::start().await;
        let user = registered(&world).await;

        let token = access_token(&world.login(&user.email, PASSWORD).await);

        let (header, claims) = jwt::decode(&token);
        assert_eq!(header["alg"], "ES256");
        assert!(!header["kid"].as_str().unwrap().trim().is_empty());
        let names: Vec<&str> = claims.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(names, ["exp", "iat", "jti", "sub"]);
        assert_eq!(claims["sub"], user.id.to_string());
        assert_eq!(claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(), 600);
    }

    /// Java: `AuthApiTest.jwksPublishesOnlyThePublicKeyThatVerifiesTokens`.
    #[tokio::test]
    async fn jwks_publishes_only_the_public_key_that_verifies_tokens() {
        let world = World::start().await;
        let token = access_token(&logged_in(&world).await);

        let jwks = world.anonymous().get("/.well-known/jwks.json").send().await;

        assert_eq!(jwks.status, 200);
        let key = &jwks.json()["keys"][0];
        assert_eq!(key["kty"], "EC");
        assert_eq!(key["crv"], "P-256");
        assert!(key.get("d").is_none(), "private key material");
        assert_eq!(key["kid"], jwt::decode(&token).0["kid"]);
    }

    /// Java: `AuthApiTest.aValidTokenIsAcceptedAndNoTokenIsNot`, the half without a token. The other half is
    /// in `m6`.
    #[tokio::test]
    async fn no_token_is_not_accepted() {
        let world = World::start().await;

        let response = world
            .anonymous()
            .get(&format!("{V1}/teams/{}/expenses", Uuid::new_v4()))
            .send()
            .await;

        assert_eq!(response.status, 401);
    }

    /// Java: `AuthApiTest.unauthenticatedResponsesAreProblemDetails`.
    #[tokio::test]
    async fn unauthenticated_responses_are_problem_details() {
        let world = World::start().await;

        let response = world
            .anonymous()
            .get(&format!("{V1}/teams/{}/expenses", Uuid::new_v4()))
            .send()
            .await;

        assert_eq!(response.problem_code().as_deref(), Some("UNAUTHENTICATED"));
        assert!(response.header("www-authenticate").unwrap().starts_with("Bearer"));
        assert!(
            response
                .header("content-type")
                .unwrap()
                .contains("application/problem+json")
        );
    }

    /// Java: `AuthApiTest.tamperedTokenGets401`.
    #[tokio::test]
    async fn tampered_token_gets_401() {
        let world = World::start().await;
        let token = access_token(&logged_in(&world).await);

        // Another subject in the payload, with the original signature.
        let tampered = jwt::with_another_subject(&token);

        assert_eq!(protected_status_with(&world, &tampered).await, 401);
    }

    /// Java: `AuthApiTest.algNoneTokenGets401`.
    #[tokio::test]
    async fn alg_none_token_gets_401() {
        let world = World::start().await;
        let now = chrono::Utc::now().timestamp();
        let claims = json!({"sub": Uuid::new_v4(), "iat": now, "exp": now + 600});

        assert_eq!(protected_status_with(&world, &jwt::unsigned(&claims)).await, 401);
    }

    /// Java: `AuthApiTest.tokenSignedWithAnotherKeyGets401`.
    #[tokio::test]
    async fn token_signed_with_another_key_gets_401() {
        let world = World::start().await;

        let token = jwt::es256_with_an_attackers_key(&jwt::claims_for(Uuid::new_v4()));

        assert_eq!(protected_status_with(&world, &token).await, 401);
    }

    /// Java: `AuthApiTest.algorithmConfusionWithAnHmacTokenGets401`.
    #[tokio::test]
    async fn algorithm_confusion_with_an_hmac_token_gets_401() {
        let world = World::start().await;
        // The classic attack: sign with HS256 using the (public) key bytes as the secret.
        let jwks = world.anonymous().get("/.well-known/jwks.json").send().await.json();
        let secret = jwks["keys"][0]["x"].as_str().unwrap().repeat(2);

        let token = jwt::hs256(&jwt::claims_for(Uuid::new_v4()), secret.as_bytes());

        assert_eq!(protected_status_with(&world, &token).await, 401);
    }

    /// Java: `AuthApiTest.garbageBearerValuesGet401`.
    #[tokio::test]
    async fn garbage_bearer_values_get_401() {
        let world = World::start().await;

        for garbage in ["abc", "a.b.c", "....", "Bearer"] {
            assert_eq!(protected_status_with(&world, garbage).await, 401, "{garbage}");
        }
    }

    // --- refresh and logout (API2) ---

    /// Java: `AuthApiTest.refreshRotatesTheTokenAndIssuesANewAccessToken`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn refresh_rotates_the_token_and_issues_a_new_access_token() {
        let world = World::start().await;
        let login = logged_in(&world).await;

        let refreshed = world.refresh(&cookie(&login)).await;

        assert_eq!(refreshed.status, 200);
        assert_ne!(cookie(&refreshed), cookie(&login));
        assert!(!access_token(&refreshed).trim().is_empty());
        let set_cookie = refreshed.header("set-cookie").unwrap();
        assert!(
            set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Strict"),
            "{set_cookie}"
        );
        // The new cookie keeps working.
        assert_eq!(world.refresh(&cookie(&refreshed)).await.status, 200);
    }

    /// Java: `AuthApiTest.replayingARotatedTokenRevokesTheWholeFamily`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn replaying_a_rotated_token_revokes_the_whole_family() {
        let world = World::start().await;
        let login = logged_in(&world).await;
        let refreshed = world.refresh(&cookie(&login)).await;
        assert_eq!(refreshed.status, 200);

        let replay = world.refresh(&cookie(&login)).await;

        assert_eq!(replay.status, 401);
        // The attacker's replay killed the legitimate session too: the successor is dead.
        assert_eq!(world.refresh(&cookie(&refreshed)).await.status, 401);
    }

    /// Java: `AuthApiTest.refreshTokensAreStoredOnlyAsHashes`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn refresh_tokens_are_stored_only_as_hashes() {
        let world = World::start().await;
        let login = logged_in(&world).await;

        let matches: i64 = sqlx::query_scalar("select count(*) from refresh_tokens where token_hash = $1")
            .bind(cookie(&login))
            .fetch_one(world.fixtures.pool())
            .await
            .unwrap();

        assert_eq!(matches, 0);
    }

    /// Java: `AuthApiTest.refreshNeedsTheCustomHeaderAndTheCookie`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn refresh_needs_the_custom_header_and_the_cookie() {
        let world = World::start().await;
        let login = logged_in(&world).await;
        let refresh = format!("{V1}/auth/refresh");
        let cookie_header = format!("refresh_token={}", cookie(&login));

        // Missing X-Requested-With: the CSRF defence of Java's ADR 0004.
        let no_header = world
            .anonymous()
            .post(&refresh)
            .header("Cookie", &cookie_header)
            .send()
            .await;
        let wrong_header = world
            .anonymous()
            .post(&refresh)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Cookie", &cookie_header)
            .send()
            .await;
        let no_cookie = world
            .anonymous()
            .post(&refresh)
            .header("X-Requested-With", "ledgerly")
            .send()
            .await;

        assert_eq!(no_header.status, 400);
        assert_eq!(wrong_header.status, 400);
        assert_eq!(no_cookie.status, 401);
        // The rejected attempts did not consume the token.
        assert_eq!(world.refresh(&cookie(&login)).await.status, 200);
    }

    /// Java: `AuthApiTest.unknownRefreshTokenGets401`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn unknown_refresh_token_gets_401() {
        let world = World::start().await;

        assert_eq!(world.refresh(&"a".repeat(43)).await.status, 401);
    }

    /// Java: `AuthApiTest.logoutRevokesTheFamilyAndClearsTheCookie`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn logout_revokes_the_family_and_clears_the_cookie() {
        let world = World::start().await;
        let login = logged_in(&world).await;
        let refreshed = world.refresh(&cookie(&login)).await;

        let out = world.logout(&cookie(&refreshed)).await;

        assert_eq!(out.status, 204);
        let set_cookie = out.header("set-cookie").unwrap();
        assert!(
            set_cookie.contains("Max-Age=0") && set_cookie.contains("Path=/api/v1/auth"),
            "{set_cookie}"
        );
        assert_eq!(world.refresh(&cookie(&refreshed)).await.status, 401);
    }

    /// Java: `AuthApiTest.logoutIsIdempotentButStillNeedsTheCustomHeader`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn logout_is_idempotent_but_still_needs_the_custom_header() {
        let world = World::start().await;
        let logout = format!("{V1}/auth/logout");

        let with_header = world
            .anonymous()
            .post(&logout)
            .header("X-Requested-With", "ledgerly")
            .send()
            .await;
        let without_header = world.anonymous().post(&logout).send().await;

        assert_eq!(with_header.status, 204);
        assert_eq!(without_header.status, 400);
    }

    // --- log hygiene (API2) ---

    /// Java: `AuthApiTest.logsNeverContainPasswordsOrTokens`. The log is the stdout of the app's container.
    #[tokio::test]
    #[ignore = "M5"]
    async fn logs_never_contain_passwords_or_tokens() {
        let world = World::start().await;
        let user = registered(&world).await;
        let verification_token = world.fixtures.verification_token(&user).await;
        assert_eq!(world.verify(&verification_token).await.status, 204);
        let login = world.login(&user.email, PASSWORD).await;
        world.login(&user.email, "Wrong-Passw0rd-abc").await;
        let refreshed = world.refresh(&cookie(&login)).await;
        world.refresh(&cookie(&login)).await; // replay: logs a warning about reuse
        world.logout(&cookie(&refreshed)).await;

        let logs = world.target.app_logs();

        for secret in [
            PASSWORD,
            "Wrong-Passw0rd-abc",
            &verification_token,
            &access_token(&login),
            &access_token(&refreshed),
            &cookie(&login),
            &cookie(&refreshed),
        ] {
            assert!(!logs.contains(secret), "the logs contain {secret}");
        }
    }
}

mod m6 {
    use super::*;

    /// Java: `AuthApiTest.aValidTokenIsAcceptedAndNoTokenIsNot`, the half with a token: 404, not 401, because
    /// the caller is authenticated but not a teammate of this (random) team.
    #[tokio::test]
    #[ignore = "M6"]
    async fn a_valid_token_is_accepted() {
        let world = World::start().await;
        let token = access_token(&logged_in(&world).await);

        assert_eq!(protected_status_with(&world, &token).await, 404);
    }
}
