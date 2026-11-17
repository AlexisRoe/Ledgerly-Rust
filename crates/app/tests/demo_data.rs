//! The `local` demo data (plan M5.8, `docs/reference/demo-data.md`).
//!
//! Java: `shared/seed/DevDataSeederTest.java`.

// clippy.toml exempts `#[test]` functions only; the helpers of an integration test are test code too.
#![allow(clippy::unwrap_used)]

use chrono::{DateTime, NaiveDate, TimeDelta, TimeZone, Utc};
use ledgerly_app::seed::{self, Seeded, TEAM};
use ledgerly_shared::config::Profiles;
use ledgerly_shared::database::TestDatabase;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// claimant, amount, currency, category, incurred on, merchant, status, version
type ExpenseRow = (String, Decimal, String, String, NaiveDate, String, String, i64);

fn local() -> Profiles {
    Profiles {
        local: true,
        ..Profiles::default()
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 13, 12, 0, 0).unwrap()
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

async fn id_of(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("select id from users where email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_empty_database_gets_exactly_the_demo_data() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();

    assert_eq!(seed::demo_data(local(), pool, now()).await.unwrap(), Seeded::Created);

    let users: Vec<(String, bool, String)> =
        sqlx::query_as("select email, verified, password_hash from users order by email")
            .fetch_all(pool)
            .await
            .unwrap();
    let emails: Vec<(&str, bool)> = users.iter().map(|(e, v, _)| (e.as_str(), *v)).collect();
    assert_eq!(
        emails,
        [
            ("alice@example.com", true),
            ("bob@example.com", true),
            ("carol@example.com", true),
            ("dave@example.com", false)
        ]
    );
    // One hash for all, Spring's format.
    assert!(users.iter().all(|(_, _, hash)| *hash == users[0].2));
    assert!(users[0].2.starts_with("$argon2id$v=19$m=16384,t=2,p=1$"));

    let team: (String, DateTime<Utc>) = sqlx::query_as("select name, created_at from teams where id = $1")
        .bind(TEAM)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(team, ("Acme Travel".to_owned(), now()));
    assert_eq!(TEAM.to_string(), "00000000-0000-7000-8000-00000000a001");

    let roles: Vec<(String, String)> = sqlx::query_as(
        "select u.email, m.role from memberships m join users u on u.id = m.user_id where m.team_id = $1 \
         order by u.email",
    )
    .bind(TEAM)
    .fetch_all(pool)
    .await
    .unwrap();
    let roles: Vec<(&str, &str)> = roles.iter().map(|(e, r)| (e.as_str(), r.as_str())).collect();
    assert_eq!(
        roles,
        [
            ("alice@example.com", "OWNER"),
            ("bob@example.com", "APPROVER"),
            ("carol@example.com", "MEMBER")
        ]
    );

    let invitation: (String, String, String, Uuid, DateTime<Utc>) =
        sqlx::query_as("select email, role, status, invited_by, expires_at from invitations where team_id = $1")
            .bind(TEAM)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        invitation,
        (
            "dave@example.com".to_owned(),
            "MEMBER".to_owned(),
            "PENDING".to_owned(),
            id_of(pool, "alice@example.com").await,
            now() + TimeDelta::days(7)
        )
    );

    let expenses: Vec<ExpenseRow> = sqlx::query_as(
        "select u.email, e.amount, e.currency, e.category, e.incurred_on, e.merchant, e.status, e.version \
         from expenses e join users u on u.id = e.claimant_id where e.team_id = $1 order by e.incurred_on desc",
    )
    .bind(TEAM)
    .fetch_all(pool)
    .await
    .unwrap();
    let today = now().date_naive();
    let rows: Vec<(&str, String, &str, &str, i64, &str, &str)> = expenses
        .iter()
        .map(|(email, amount, currency, category, on, merchant, status, version)| {
            assert_eq!(*version, 0);
            (
                email.as_str(),
                amount.to_string(),
                currency.as_str(),
                category.as_str(),
                (today - *on).num_days(),
                merchant.as_str(),
                status.as_str(),
            )
        })
        .collect();
    let row = |e, a: &str, c, k, d, m, s| (e, a.to_owned(), c, k, d, m, s);
    assert_eq!(
        rows,
        [
            row("bob@example.com", "19.9000", "USD", "OTHER", 1, "Parking", "DRAFT"),
            row(
                "carol@example.com",
                "38.4000",
                "EUR",
                "MEALS",
                2,
                "Cafe Central",
                "DRAFT"
            ),
            row(
                "alice@example.com",
                "120.0000",
                "EUR",
                "MEALS",
                4,
                "Team dinner",
                "SUBMITTED"
            ),
            row(
                "carol@example.com",
                "212.0000",
                "EUR",
                "TRAVEL",
                6,
                "Rail Europe",
                "SUBMITTED"
            ),
            row(
                "carol@example.com",
                "49.9900",
                "EUR",
                "SOFTWARE",
                9,
                "Editor Inc",
                "APPROVED"
            ),
            row(
                "carol@example.com",
                "980.0000",
                "EUR",
                "OFFICE",
                12,
                "Desk Depot",
                "REJECTED"
            ),
            row(
                "carol@example.com",
                "75.0000",
                "EUR",
                "TRAVEL",
                20,
                "City Taxi",
                "REIMBURSED"
            ),
        ]
    );

    let rejection: (String, i32, Uuid, String) = sqlx::query_as(
        "select e.merchant, r.position, r.rejected_by, r.reason from expense_rejections r \
         join expenses e on e.id = r.expense_id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        rejection,
        (
            "Desk Depot".to_owned(),
            0,
            id_of(pool, "bob@example.com").await,
            "Over the office budget; please get a quote first.".to_owned()
        )
    );
    assert_eq!(count(pool, "select count(*) from receipts").await, 0);
    assert_eq!(count(pool, "select count(*) from event_publication").await, 0);
}

#[tokio::test]
async fn a_second_start_adds_nothing() {
    let database = TestDatabase::new().await.unwrap();

    seed::demo_data(local(), database.pool(), now()).await.unwrap();
    let again = seed::demo_data(local(), database.pool(), now() + TimeDelta::days(1))
        .await
        .unwrap();

    assert_eq!(again, Seeded::AlreadyThere);
    assert_eq!(count(database.pool(), "select count(*) from expenses").await, 7);
    assert_eq!(count(database.pool(), "select count(*) from users").await, 4);
}

#[tokio::test]
async fn an_existing_account_keeps_its_id_and_password() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let existing = Uuid::now_v7();
    sqlx::query(
        "insert into users (id, email, password_hash, verified, created_at) \
         values ($1, 'bob@example.com', 'kept', false, now())",
    )
    .bind(existing)
    .execute(pool)
    .await
    .unwrap();

    seed::demo_data(local(), pool, now()).await.unwrap();

    let (id, hash): (Uuid, String) =
        sqlx::query_as("select id, password_hash from users where email = 'bob@example.com'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!((id, hash.as_str()), (existing, "kept"));
}

#[tokio::test]
async fn only_local_without_test_and_without_keycloak_seeds() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let with = |change: fn(&mut Profiles)| {
        let mut profiles = local();
        change(&mut profiles);
        profiles
    };

    assert_eq!(
        seed::demo_data(Profiles::default(), pool, now()).await.unwrap(),
        Seeded::NotLocal
    );
    assert_eq!(
        seed::demo_data(with(|p| p.test = true), pool, now()).await.unwrap(),
        Seeded::NotLocal
    );
    assert_eq!(
        seed::demo_data(with(|p| p.keycloak = true), pool, now()).await.unwrap(),
        Seeded::SkippedForKeycloak
    );
    assert_eq!(count(pool, "select count(*) from users").await, 0);
}
