//! The audit trail: what is recorded, who may read it, and cursor paging.
//!
//! Java: `src/test/java/com/ledgerly/audit/internal/web/AuditApiTest.java`. Its `owner`, `approver` and
//! `member` are the team's roles here.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use ledgerly_parity_tests::fixtures::User;
use ledgerly_parity_tests::http::{Client, Response};
use ledgerly_parity_tests::{SignedInTeam, V1, World, eventually, expense_json};
use serde_json::{Value, json};
use uuid::Uuid;

async fn trail(client: &Client, team: Uuid, query: &str) -> Response {
    client
        .get(&format!("{V1}/teams/{team}/audit-events{query}"))
        .send()
        .await
}

fn types(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .unwrap_or_else(|| panic!("no items in {body}"))
        .iter()
        .map(|item| item["eventType"].as_str().unwrap().to_owned())
        .collect()
}

/// Java: `awaitTrail()`: events are written asynchronously, so poll for up to 30 seconds.
async fn await_trail(owner: &Client, team: Uuid, wanted: &str) -> Value {
    eventually(&format!("{wanted} in the trail"), Duration::from_secs(30), || async {
        let response = trail(owner, team, "?limit=100").await;
        let body = response.json();
        (response.status == 200 && types(&body).iter().any(|kind| kind == wanted)).then_some(body)
    })
    .await
}

/// Java: `seed()`: `count` rows of type `SEEDED`, two per second from `start`.
async fn seed(world: &World, team: Uuid, count: i64, start: DateTime<Utc>) {
    for n in 0..count {
        sqlx::query(
            "insert into audit_events (id, event_id, team_id, event_type, details, occurred_at, created_at) \
             values ($1, $2, $3, 'SEEDED', jsonb_build_object('n', $4::int), $5, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(Uuid::new_v4())
        .bind(team)
        .bind(i32::try_from(n).unwrap())
        .bind(start + chrono::Duration::seconds(n / 2))
        .execute(world.fixtures.pool())
        .await
        .unwrap();
    }
}

fn next_cursor(body: &Value) -> Option<String> {
    body.get("nextCursor").and_then(Value::as_str).map(str::to_owned)
}

fn jan_first() -> DateTime<Utc> {
    "2026-01-01T00:00:00Z".parse().unwrap()
}

async fn create(world: &World, team: &SignedInTeam) -> Response {
    world
        .create_expense(team, &team.member, &expense_json("10.00", "Cafe Central"))
        .await
}

mod m9 {
    use super::*;

    /// Java: `AuditApiTest.submittingAnExpenseLeavesARowAndTheWholeLifeIsTold`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn submitting_an_expense_leaves_a_row_and_the_whole_life_is_told() {
        let world = World::start().await;
        let team = world.team().await;
        let created = create(&world, &team).await;
        let id = created.json()["id"].as_str().unwrap().to_owned();
        let path = team.expense_path(id.parse().unwrap());
        // Without If-Match: refused with 428, and a refused request leaves no event.
        world
            .as_user(&team.member)
            .patch(&path)
            .json(&json!({"merchant": "Bakery"}))
            .send()
            .await;
        world
            .fixtures
            .accepted_receipt(team.id, id.parse().unwrap(), &team.member.user)
            .await;

        assert_eq!(
            world
                .as_user(&team.member)
                .post(&format!("{path}:submit"))
                .send()
                .await
                .status,
            200
        );
        assert_eq!(
            world
                .as_user(&team.approver)
                .post(&format!("{path}:approve"))
                .send()
                .await
                .status,
            200
        );
        assert_eq!(
            world
                .as_user(&team.owner)
                .post(&format!("{path}:reimburse"))
                .send()
                .await
                .status,
            200
        );

        let body = await_trail(&world.as_user(&team.owner), team.id, "EXPENSE_REIMBURSED").await;
        assert_eq!(
            types(&body),
            [
                "EXPENSE_REIMBURSED",
                "EXPENSE_APPROVED",
                "EXPENSE_SUBMITTED",
                "EXPENSE_CREATED"
            ]
        );
        let submitted = &body["items"][2];
        assert_eq!(submitted["actorId"], team.member.id().to_string());
        assert_eq!(submitted["subjectType"], "EXPENSE");
        assert_eq!(submitted["subjectId"], id);
        assert_eq!(body["items"][1]["actorId"], team.approver.id().to_string());
        assert_eq!(body["items"][0]["actorId"], team.owner.id().to_string());
    }

    /// Java: `AuditApiTest.anEditRecordsWhichFieldsChangedButNeverTheirValues`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn an_edit_records_which_fields_changed_but_never_their_values() {
        let world = World::start().await;
        let team = world.team().await;
        let created = create(&world, &team).await;
        let path = team.expense_path(created.json()["id"].as_str().unwrap().parse().unwrap());

        let edited = world
            .as_user(&team.member)
            .patch(&path)
            .header("If-Match", created.header("etag").unwrap())
            .json(&json!({"merchant": "Very Secret Bakery", "note": "private note"}))
            .send()
            .await;

        assert_eq!(edited.status, 200);
        let body = await_trail(&world.as_user(&team.owner), team.id, "EXPENSE_EDITED").await;
        let event = &body["items"][0];
        assert_eq!(event["eventType"], "EXPENSE_EDITED");
        // Java compares the JSON text, so the order of the names matters.
        assert_eq!(event["details"]["changedFields"].to_string(), r#"["merchant","note"]"#);
        let text = body.to_string();
        for value in ["Bakery", "private note", "Cafe"] {
            assert!(!text.contains(value), "{value} in {text}");
        }
    }

    /// Java: `AuditApiTest.teamEventsAppearToo`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn team_events_appear_too() {
        let world = World::start().await;
        let team = world.team().await;
        let owner = world.as_user(&team.owner);

        let created = owner
            .post(&format!("{V1}/teams"))
            .json(&json!({"name": "Acme"}))
            .send()
            .await;
        let new_team: Uuid = created.json()["id"].as_str().unwrap().parse().unwrap();

        let body = await_trail(&owner, new_team, "TEAM_CREATED").await;
        assert_eq!(body["items"][0]["subjectId"], new_team.to_string());
    }

    /// Java: `AuditApiTest.registeringAndVerifyingAreStoredWithoutATeamAndNeverShownInOne`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn registering_and_verifying_are_stored_without_a_team_and_never_shown_in_one() {
        let world = World::start().await;
        let email = format!("audit-{}@example.com", Uuid::new_v4());
        assert_eq!(world.register(&email, "S3cret-Passw0rd-xyz").await.status, 202);
        let user = User {
            id: world.user_id_of(&email).await,
            email: email.clone(),
        };
        let token = world.fixtures.verification_token(&user).await;
        world.verify(&token).await;

        eventually(
            "USER_REGISTERED and USER_VERIFIED without a team",
            Duration::from_secs(30),
            || async {
                let rows: i64 = sqlx::query_scalar(
                    "select count(*) from audit_events where team_id is null and actor_id = $1 \
                 and event_type in ('USER_REGISTERED', 'USER_VERIFIED')",
                )
                .bind(user.id)
                .fetch_one(world.fixtures.pool())
                .await
                .unwrap();
                (rows == 2).then_some(())
            },
        )
        .await;
        let users_team = world.fixtures.team().await;
        world.fixtures.member(users_team, &user, "OWNER").await;
        let session = world.login(&email, "S3cret-Passw0rd-xyz").await.json()["accessToken"]
            .as_str()
            .unwrap()
            .to_owned();

        let response = trail(&world.with_token(&session), users_team, "").await;

        assert_eq!(response.status, 200);
        let kinds = types(&response.json());
        assert!(
            !kinds
                .iter()
                .any(|kind| kind == "USER_REGISTERED" || kind == "USER_VERIFIED"),
            "{kinds:?}"
        );
    }

    /// Java: `AuditApiTest.onlyOwnersMayReadTheTrail`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn only_owners_may_read_the_trail() {
        let world = World::start().await;
        let team = world.team().await;
        let outsider = world.user().await;

        for (client, expected) in [
            (world.as_user(&team.owner), 200),
            (world.as_user(&team.approver), 403),
            (world.as_user(&team.member), 403),
            (world.as_user(&outsider), 404),
            (world.anonymous(), 401),
        ] {
            assert_eq!(trail(&client, team.id, "").await.status, expected);
        }
    }

    /// Java: `AuditApiTest.anotherTeamsEventsAreNeverVisible_API1`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn another_teams_events_are_never_visible() {
        let world = World::start().await;
        let team = world.team().await;
        let other_owner = world.user().await;
        let other_team = world.fixtures.team().await;
        world.fixtures.member(other_team, &other_owner.user, "OWNER").await;
        sqlx::query(
            "insert into audit_events (id, event_id, team_id, event_type, details, occurred_at, created_at) \
             values ($1, $2, $3, 'SECRET_OF_OTHER_TEAM', '{}'::jsonb, now(), now())",
        )
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(other_team)
        .execute(world.fixtures.pool())
        .await
        .unwrap();

        let mine = types(&trail(&world.as_user(&team.owner), team.id, "").await.json());
        let theirs = types(&trail(&world.as_user(&other_owner), other_team, "").await.json());

        assert!(!mine.iter().any(|kind| kind == "SECRET_OF_OTHER_TEAM"));
        assert_eq!(theirs, ["SECRET_OF_OTHER_TEAM"]);
    }

    /// Java: `AuditApiTest.pagingWithCursorsVisitsEveryRowOnceNewestFirst`. Java's team has no other events;
    /// neither has a team written with SQL.
    #[tokio::test]
    #[ignore = "M9"]
    async fn paging_with_cursors_visits_every_row_once_newest_first() {
        let world = World::start().await;
        let team = world.team().await;
        seed(&world, team.id, 120, jan_first()).await;
        let owner = world.as_user(&team.owner);

        let (mut seen, mut occurred, mut cursor, mut pages) = (BTreeSet::new(), Vec::new(), None::<String>, 0);
        loop {
            let query = cursor
                .as_ref()
                .map_or_else(|| "?limit=50".to_owned(), |c| format!("?limit=50&after={c}"));
            let response = trail(&owner, team.id, &query).await;
            assert_eq!(response.status, 200);
            let body = response.json();
            for item in body["items"].as_array().unwrap() {
                assert!(seen.insert(item["id"].as_str().unwrap().to_owned()), "row seen twice");
                occurred.push(item["occurredAt"].as_str().unwrap().parse::<DateTime<Utc>>().unwrap());
            }
            pages += 1;
            cursor = next_cursor(&body);
            if cursor.is_none() {
                break;
            }
        }

        assert_eq!(pages, 3);
        assert_eq!(seen.len(), 120);
        assert!(occurred.windows(2).all(|pair| pair[0] >= pair[1]), "newest first");
    }

    /// Java: `AuditApiTest.newRowsArrivingWhilePagingNeverCauseRepeats`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn new_rows_arriving_while_paging_never_cause_repeats() {
        let world = World::start().await;
        let team = world.team().await;
        seed(&world, team.id, 60, jan_first()).await;
        let owner = world.as_user(&team.owner);
        let first = trail(&owner, team.id, "?limit=30").await.json();
        let mut seen: BTreeSet<String> = first["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap().to_owned())
            .collect();
        let mut cursor = next_cursor(&first);

        // Newer than everything, arriving mid-way.
        seed(&world, team.id, 10, Utc::now() + chrono::Duration::hours(1)).await;
        while let Some(after) = cursor {
            let body = trail(&owner, team.id, &format!("?limit=30&after={after}")).await.json();
            for item in body["items"].as_array().unwrap() {
                assert!(seen.insert(item["id"].as_str().unwrap().to_owned()), "row seen twice");
            }
            cursor = next_cursor(&body);
        }

        assert_eq!(seen.len(), 60);
    }

    /// Java: `AuditApiTest.theLastPageHasNoCursor`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn the_last_page_has_no_cursor() {
        let world = World::start().await;
        let team = world.team().await;
        seed(&world, team.id, 3, jan_first()).await;

        let body = trail(&world.as_user(&team.owner), team.id, "?limit=50").await.json();

        assert_eq!(body["items"].as_array().unwrap().len(), 3);
        assert_eq!(next_cursor(&body), None);
    }

    /// Java: `AuditApiTest.badCursorsAndLimitsGet400`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn bad_cursors_and_limits_get_400() {
        let world = World::start().await;
        let team = world.team().await;
        let owner = world.as_user(&team.owner);

        for query in ["?after=not-a-cursor", "?after=e30", "?limit=0", "?limit=101"] {
            assert_eq!(trail(&owner, team.id, query).await.status, 400, "{query}");
        }
    }
}
