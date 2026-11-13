//! Keycloak instead of Ledgerly's own login (profile `keycloak`).
//!
//! Java: `src/test/java/com/ledgerly/identity/internal/keycloak/KeycloakEndToEndTest.java`, and the HTTP part
//! of `ExternalIdentityTest.ourOwnLoginRegistrationAndKeyEndpointsAreGone`. This file runs in the `keycloak`
//! group against the Keycloak of `compose.yaml` with the vendored realm.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::time::Duration;

use ledgerly_parity_tests::{V1, World, eventually, expense_json, jwt};
use serde_json::{Value, json};
use uuid::Uuid;

/// The password grant, enabled in the development realm only; a real client uses the authorization code flow.
async fn token_for(world: &World, client_id: &str, username: &str, password: &str) -> String {
    let response = reqwest::Client::new()
        .post(format!(
            "{}/realms/ledgerly/protocol/openid-connect/token",
            world.target.keycloak_url
        ))
        .form(&[
            ("grant_type", "password"),
            ("scope", "openid email"),
            ("client_id", client_id),
            ("username", username),
            ("password", password),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    body["access_token"].as_str().unwrap().to_owned()
}

async fn alice(world: &World) -> String {
    token_for(world, "ledgerly-web", "alice@example.com", "alice-password").await
}

fn claims(token: &str) -> Value {
    jwt::decode(token).1
}

async fn me(world: &World, token: &str) -> Value {
    world.with_token(token).get(&format!("{V1}/me")).send().await.json()
}

async fn user_count(world: &World, email: &str) -> i64 {
    sqlx::query_scalar("select count(*) from users where email = $1")
        .bind(email)
        .fetch_one(world.fixtures.pool())
        .await
        .unwrap()
}

async fn external_subject(world: &World, id: &str) -> Option<String> {
    sqlx::query_scalar("select external_subject from users where id = $1")
        .bind(id.parse::<Uuid>().unwrap())
        .fetch_one(world.fixtures.pool())
        .await
        .unwrap()
}

mod m11 {
    use super::*;

    /// Java: `KeycloakEndToEndTest.aKeycloakTokenWorksAndCreatesOurUserOnFirstUse`.
    #[tokio::test]
    #[ignore = "M11"]
    async fn a_keycloak_token_works_and_creates_our_user_on_first_use() {
        let world = World::start().await;
        let token = alice(&world).await;
        let keycloak_sub = claims(&token)["sub"].as_str().unwrap().to_owned();
        assert!(claims(&token)["aud"].to_string().contains("ledgerly-api"));
        // Tests share one database: another test may already have created or linked Alice.
        let created_here = user_count(&world, "alice@example.com").await == 0;

        let response = world.with_token(&token).get(&format!("{V1}/me")).send().await;

        assert_eq!(response.status, 200);
        let body = response.json();
        assert_eq!(body["email"], "alice@example.com");
        assert_eq!(body["verified"], true);
        let our_id = body["id"].as_str().unwrap().to_owned();
        assert_ne!(our_id, keycloak_sub, "Ledgerly's own id, not Keycloak's");
        assert_eq!(
            external_subject(&world, &our_id).await.as_deref(),
            Some(keycloak_sub.as_str())
        );
        if created_here {
            let hash: Option<String> = sqlx::query_scalar("select password_hash from users where id = $1")
                .bind(our_id.parse::<Uuid>().unwrap())
                .fetch_one(world.fixtures.pool())
                .await
                .unwrap();
            assert_eq!(hash, None, "just-in-time users never get a password");
        }
        // And the next call is the same person.
        assert_eq!(me(&world, &token).await["id"], our_id);
    }

    /// Java: `KeycloakEndToEndTest.theRestOfLedgerlyWorksUnchangedWithKeycloakIdentities`.
    #[tokio::test]
    #[ignore = "M11"]
    async fn the_rest_of_ledgerly_works_unchanged_with_keycloak_identities() {
        let world = World::start().await;
        let token = alice(&world).await;
        let client = world.with_token(&token);
        let our_id = me(&world, &token).await["id"].as_str().unwrap().to_owned();

        let team = client
            .post(&format!("{V1}/teams"))
            .json(&json!({"name": "Keycloak team"}))
            .send()
            .await;
        assert_eq!(team.status, 201);
        let team_id = team.json()["id"].as_str().unwrap().to_owned();
        let created = client
            .post(&format!("{V1}/teams/{team_id}/expenses"))
            .json(&expense_json("12.50", "Cafe"))
            .send()
            .await;
        assert_eq!(created.status, 201);
        assert_eq!(created.json()["claimantId"], our_id);

        eventually(
            "TEAM_CREATED and EXPENSE_CREATED in the trail",
            Duration::from_secs(20),
            || async {
                let trail = client.get(&format!("{V1}/teams/{team_id}/audit-events")).send().await;
                let body = trail.json();
                let kinds: Vec<&str> = body["items"]
                    .as_array()?
                    .iter()
                    .filter_map(|item| item["eventType"].as_str())
                    .collect();
                (trail.status == 200
                    && kinds.contains(&"TEAM_CREATED")
                    && kinds.contains(&"EXPENSE_CREATED")
                    && body["items"][0]["actorId"] == our_id)
                    .then_some(())
            },
        )
        .await;
    }

    /// Java: `KeycloakEndToEndTest.anUnverifiedEmailStaysUnverifiedAndCannotAcceptInvitations`.
    #[tokio::test]
    #[ignore = "M11"]
    async fn an_unverified_email_stays_unverified_and_cannot_accept_invitations() {
        let world = World::start().await;
        let alice = alice(&world).await;
        let bob = token_for(&world, "ledgerly-web", "bob@example.com", "bob-password").await;
        assert_eq!(claims(&bob)["email_verified"], false);
        assert_eq!(me(&world, &bob).await["verified"], false);
        let team_id = world
            .with_token(&alice)
            .post(&format!("{V1}/teams"))
            .json(&json!({"name": "Invite bob"}))
            .send()
            .await
            .json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let invitation = world
            .with_token(&alice)
            .post(&format!("{V1}/teams/{team_id}/invitations"))
            .json(&json!({"email": "bob@example.com", "role": "MEMBER"}))
            .send()
            .await
            .json();

        let accept = world
            .with_token(&bob)
            .post(&format!(
                "{V1}/invitations/{}:accept",
                invitation["id"].as_str().unwrap()
            ))
            .send()
            .await;

        assert_eq!(accept.status, 403);
        assert_eq!(accept.problem_code().as_deref(), Some("EMAIL_NOT_VERIFIED"));
        // Unverified: the invitation is hidden.
        assert_eq!(me(&world, &bob).await["invitations"], json!([]));
    }

    /// Java: `KeycloakEndToEndTest.aTokenForAnotherApplicationOfTheSameRealmIsRefused`.
    #[tokio::test]
    #[ignore = "M11"]
    async fn a_token_for_another_application_of_the_same_realm_is_refused() {
        let world = World::start().await;

        let foreign = token_for(&world, "other-app", "alice@example.com", "alice-password").await;

        assert!(
            !claims(&foreign)
                .get("aud")
                .map(Value::to_string)
                .unwrap_or_default()
                .contains("ledgerly-api")
        );
        assert_eq!(
            world.with_token(&foreign).get(&format!("{V1}/me")).send().await.status,
            401
        );
    }

    /// Java: `KeycloakEndToEndTest.tamperedAndGarbageTokensAreRefused`.
    #[tokio::test]
    #[ignore = "M11"]
    async fn tampered_and_garbage_tokens_are_refused() {
        let world = World::start().await;
        let tampered = jwt::with_another_subject(&alice(&world).await);
        let me_path = format!("{V1}/me");

        assert_eq!(world.with_token(&tampered).get(&me_path).send().await.status, 401);
        assert_eq!(world.with_token("not.a.jwt").get(&me_path).send().await.status, 401);
        assert_eq!(world.anonymous().get(&me_path).send().await.status, 401);
    }

    /// Java: `KeycloakEndToEndTest.anExistingLocalAccountWithTheSameVerifiedEmailIsLinkedNotDuplicated`.
    #[tokio::test]
    #[ignore = "M11"]
    async fn an_existing_local_account_with_the_same_verified_email_is_linked_not_duplicated() {
        let world = World::start().await;
        // A person who registered with Ledgerly's own login before the switch.
        sqlx::query(
            "insert into users (id, email, password_hash, verified, created_at) \
             values ($1, 'alice@example.com', 'old-hash', true, now()) on conflict (email) do nothing",
        )
        .bind(Uuid::new_v4())
        .execute(world.fixtures.pool())
        .await
        .unwrap();
        let token = alice(&world).await;

        let our_id = me(&world, &token).await["id"].as_str().unwrap().to_owned();

        assert_eq!(user_count(&world, "alice@example.com").await, 1);
        assert_eq!(
            external_subject(&world, &our_id).await,
            claims(&token)["sub"].as_str().map(str::to_owned)
        );
    }

    /// Java: `KeycloakEndToEndTest.theApplicationNoLongerIssuesOrAcceptsItsOwnLogins` and
    /// `ExternalIdentityTest.ourOwnLoginRegistrationAndKeyEndpointsAreGone`.
    #[tokio::test]
    #[ignore = "M11"]
    async fn the_application_no_longer_issues_or_accepts_its_own_logins() {
        let world = World::start().await;
        let credentials = json!({"email": "alice@example.com", "password": "alice-password"});

        for path in ["login", "register", "refresh", "logout", "verify"] {
            let response = world
                .anonymous()
                .post(&format!("{V1}/auth/{path}"))
                .json(&credentials)
                .send_unchecked()
                .await;
            assert_eq!(response.status, 404, "{path}");
        }
        assert_eq!(
            world
                .anonymous()
                .get("/.well-known/jwks.json")
                .send_unchecked()
                .await
                .status,
            404
        );
    }
}
