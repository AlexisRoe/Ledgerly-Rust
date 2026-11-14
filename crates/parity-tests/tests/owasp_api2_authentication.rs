//! OWASP API2:2023 Broken Authentication: refresh-token replay, lockout, enumeration, password storage and
//! token tampering.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api2BrokenAuthenticationTest.java`.
//!
//! Not here, because they need a clock (see `NOT_PARITY.md`): the lock ending after 16 minutes, and a genuine
//! token being refused after 11 minutes.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::{V1, World};
use serde_json::json;
use uuid::Uuid;

/// Java: `Api2BrokenAuthenticationTest.PASSWORD`.
const PASSWORD: &str = "S3cret-Passw0rd-xyz";

mod m5 {
    use super::*;

    /// Java: `Api2BrokenAuthenticationTest.aReplayedRefreshTokenRevokesTheWholeSessionFamily`.
    #[tokio::test]
    async fn a_replayed_refresh_token_revokes_the_whole_session_family() {
        let world = World::start().await;
        let user = world.register_and_verify(PASSWORD).await;
        let login = world.login(&user.email, PASSWORD).await;
        let first_cookie = login.cookie("refresh_token").unwrap();

        let rotated = world.refresh(&first_cookie).await;
        assert_eq!(rotated.status, 200);
        let second_cookie = rotated.cookie("refresh_token").unwrap();

        // The stolen, already used token ...
        assert_eq!(world.refresh(&first_cookie).await.status, 401);
        // ... and the honest session dies with it.
        assert_eq!(world.refresh(&second_cookie).await.status, 401);
    }

    /// Java: `Api2BrokenAuthenticationTest.failedLoginsLockTheAccountEvenAgainstTheRightPassword`, without its
    /// last step (the lock ends after 16 minutes), which needs a clock.
    #[tokio::test]
    async fn failed_logins_lock_the_account_even_against_the_right_password() {
        let world = World::start().await;
        let user = world.register_and_verify(PASSWORD).await;

        for _ in 0..5 {
            assert_eq!(world.login(&user.email, "wrong-password-123").await.status, 401);
        }

        assert_eq!(world.login(&user.email, PASSWORD).await.status, 401);
    }

    /// Java: `Api2BrokenAuthenticationTest.unknownEmailAndWrongPasswordAreIndistinguishable`.
    #[tokio::test]
    async fn unknown_email_and_wrong_password_are_indistinguishable() {
        let world = World::start().await;
        let user = world.register_and_verify(PASSWORD).await;

        let wrong = world.login(&user.email, "wrong-password-123").await;
        let unknown = world
            .login(&format!("nobody-{}@example.com", Uuid::new_v4()), "wrong-password-123")
            .await;

        assert_eq!(wrong.status, unknown.status);
        assert_eq!(wrong.json()["detail"], unknown.json()["detail"]);
        assert_eq!(wrong.json()["code"], unknown.json()["code"]);
    }

    /// Java: `Api2BrokenAuthenticationTest.registeringAnExistingEmailLooksLikeRegisteringANewOne`.
    #[tokio::test]
    async fn registering_an_existing_email_looks_like_registering_a_new_one() {
        let world = World::start().await;
        let user = world.register_and_verify(PASSWORD).await;

        let again = world.register(&user.email, PASSWORD).await;
        let fresh = world
            .register(&format!("fresh-{}@example.com", Uuid::new_v4()), PASSWORD)
            .await;

        assert_eq!(again.status, fresh.status);
        assert_eq!(again.json(), fresh.json());
    }

    /// Java: `Api2BrokenAuthenticationTest.passwordsAreStoredAsArgon2idHashes`. Java reads the row through its
    /// own `JdbcClient`; the suite reads the same column.
    #[tokio::test]
    async fn passwords_are_stored_as_argon2id_hashes() {
        let world = World::start().await;
        let user = world.register_and_verify(PASSWORD).await;

        let hash: String = sqlx::query_scalar("select password_hash from users where id = $1")
            .bind(user.id)
            .fetch_one(world.fixtures.pool())
            .await
            .unwrap();

        assert!(hash.starts_with("$argon2id$"), "{hash}");
        assert!(!hash.contains(PASSWORD));
    }
}

mod m6 {
    use super::*;

    /// Java: `Api2BrokenAuthenticationTest.anUnverifiedAccountCannotAcceptAnInvitation`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn an_unverified_account_cannot_accept_an_invitation() {
        let world = World::start().await;
        let team = world.team().await;
        let unverified = world.unverified_user().await;

        let invitation = world
            .as_user(&team.owner)
            .post(&format!("{}/invitations", team.path()))
            .json(&json!({"email": unverified.email(), "role": "MEMBER"}))
            .send()
            .await
            .json();
        let accept = world
            .as_user(&unverified)
            .post(&format!(
                "{V1}/invitations/{}:accept",
                invitation["id"].as_str().unwrap()
            ))
            .send()
            .await;

        assert_eq!(accept.status, 403);
        assert_eq!(accept.problem_code().as_deref(), Some("EMAIL_NOT_VERIFIED"));
    }

    /// Java: `Api2BrokenAuthenticationTest.accessTokensAreShortLivedAndTamperedOnesAreRefused`, the tampered
    /// half. The other half (a genuine token after 11 minutes) needs a clock.
    #[tokio::test]
    #[ignore = "M6"]
    async fn a_token_with_a_forged_signature_is_refused() {
        let world = World::start().await;
        let user = world.user().await;
        let parts: Vec<&str> = user.token.split('.').collect();
        let forged = format!("{}.{}.{}", parts[0], parts[1], "A".repeat(parts[2].len()));

        let response = world.with_token(&forged).get(&format!("{V1}/me")).send().await;

        assert_eq!(response.status, 401);
    }
}
