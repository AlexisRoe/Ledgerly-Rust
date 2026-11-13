//! Persistence against real Postgres, migrated by Flyway from the vendored scripts (plan M2.3).
//!
//! In-crate tests, because the persistence functions are `pub(crate)`: nothing outside the crate can reach
//! them, which is the point.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use ledgerly_shared::clock::FixedClock;
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::money::Money;
use ledgerly_shared::role::Role;
use ledgerly_shared::version::Version;

use super::*;
use crate::ReceiptSummary;
use crate::domain::{Actor, NewExpense};

fn clock() -> FixedClock {
    FixedClock::at(at(10))
}

/// 2026-11-05 at the given hour, UTC.
fn at(hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 5, hour, 0, 0).unwrap()
}

fn claimant() -> Actor {
    Actor::new(UserId::new_v7(), Role::Member)
}

fn approver() -> Actor {
    Actor::new(UserId::new_v7(), Role::Approver)
}

fn draft(team: TeamId, claimant: Actor) -> Expense {
    let new = NewExpense {
        money: Money::parse("12.5", "EUR").unwrap(),
        category: Category::Meals,
        incurred_on: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
        merchant: "Café Müller".to_owned(),
        note: Some("Team lunch".to_owned()),
    };
    Expense::draft(team, claimant.id(), &new, &clock()).unwrap()
}

async fn load(connection: &mut PgConnection, team: TeamId, id: ExpenseId) -> Versioned<Expense> {
    find(connection, team, id).await.unwrap().unwrap()
}

/// Every field the aggregate exposes, to compare two expenses that have no `PartialEq` of their own.
fn fields(expense: &Expense) -> impl PartialEq + std::fmt::Debug {
    (
        (expense.id(), expense.team_id(), expense.claimant_id(), expense.money()),
        (expense.category(), expense.incurred_on(), expense.merchant().clone()),
        (expense.note().cloned(), expense.status(), expense.rejections().to_vec()),
    )
}

#[tokio::test]
async fn an_expense_round_trips() {
    let database = TestDatabase::new().await.unwrap();
    let mut connection = database.pool().acquire().await.unwrap();
    let team = TeamId::new_v7();
    let expense = draft(team, claimant());

    let version = insert(&mut connection, &expense, at(10)).await.unwrap();
    let loaded = load(&mut connection, team, expense.id()).await;

    assert_eq!(version, Version::INITIAL);
    assert_eq!(loaded.version, Version::INITIAL);
    assert_eq!(fields(&loaded.value), fields(&expense));
}

#[tokio::test]
async fn money_is_stored_as_numeric_19_4_and_char_3_and_read_back_at_the_currencys_scale() {
    let database = TestDatabase::new().await.unwrap();
    let mut connection = database.pool().acquire().await.unwrap();
    let team = TeamId::new_v7();
    let expense = draft(team, claimant());
    insert(&mut connection, &expense, at(10)).await.unwrap();

    let (amount, currency): (String, String) =
        sqlx::query_as("select amount::text, currency from expenses where id = $1")
            .bind(expense.id().as_uuid())
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!((amount.as_str(), currency.as_str()), ("12.5000", "EUR"));

    let loaded = load(&mut connection, team, expense.id()).await;
    assert_eq!(loaded.value.money().amount().to_string(), "12.50");
}

#[tokio::test]
async fn rejections_keep_their_order_and_their_own_reasons() {
    let database = TestDatabase::new().await.unwrap();
    let mut connection = database.pool().acquire().await.unwrap();
    let team = TeamId::new_v7();
    let (claimant, approver) = (claimant(), approver());
    let mut expense = draft(team, claimant);
    let mut version = insert(&mut connection, &expense, at(10)).await.unwrap();

    for reason in ["Receipt unreadable", "Wrong category"] {
        expense.submit(claimant, ReceiptSummary::new(1, 0)).unwrap();
        expense.reject(approver, reason).unwrap();
        version = update(&mut connection, &expense, version, at(11)).await.unwrap();
        expense.reopen(claimant).unwrap();
        version = update(&mut connection, &expense, version, at(12)).await.unwrap();
    }

    let loaded = load(&mut connection, team, expense.id()).await;
    assert_eq!(loaded.version, Version::from_stored(4));
    let reasons: Vec<&str> = loaded
        .value
        .rejections()
        .iter()
        .map(|rejection| rejection.reason().as_str())
        .collect();
    assert_eq!(reasons, ["Receipt unreadable", "Wrong category"]);
    assert!(
        loaded
            .value
            .rejections()
            .iter()
            .all(|rejection| rejection.rejected_by() == approver.id())
    );
    let positions: Vec<i32> =
        sqlx::query_scalar("select position from expense_rejections where expense_id = $1 order by position")
            .bind(expense.id().as_uuid())
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    assert_eq!(positions, [0, 1], "Hibernate's @OrderColumn counts from 0");
}

#[tokio::test]
async fn an_update_moves_the_version_and_updated_at_but_not_created_at() {
    let database = TestDatabase::new().await.unwrap();
    let mut connection = database.pool().acquire().await.unwrap();
    let team = TeamId::new_v7();
    let claimant = claimant();
    let mut expense = draft(team, claimant);
    insert(&mut connection, &expense, at(10)).await.unwrap();

    expense.submit(claimant, ReceiptSummary::new(1, 0)).unwrap();
    let version = update(&mut connection, &expense, Version::INITIAL, at(11))
        .await
        .unwrap();

    assert_eq!(version, Version::from_stored(1));
    let (created_at, updated_at, status): (DateTime<Utc>, DateTime<Utc>, String) =
        sqlx::query_as("select created_at, updated_at, status from expenses where id = $1")
            .bind(expense.id().as_uuid())
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!((created_at, updated_at, status.as_str()), (at(10), at(11), "SUBMITTED"));
}

#[tokio::test]
async fn the_second_of_two_updates_from_the_same_version_conflicts() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let team = TeamId::new_v7();
    let claimant = claimant();
    let expense = draft(team, claimant);
    insert(&mut pool.acquire().await.unwrap(), &expense, at(10))
        .await
        .unwrap();

    // Two requests read the same version, each in its own transaction.
    let mut first = pool.begin().await.unwrap();
    let mut second = pool.begin().await.unwrap();
    let mut by_first = load(&mut first, team, expense.id()).await;
    let by_second = load(&mut second, team, expense.id()).await;
    by_first.value.submit(claimant, ReceiptSummary::new(1, 0)).unwrap();
    by_second.value.require_deletable_by(claimant).unwrap();

    update(&mut first, &by_first.value, by_first.version, at(11))
        .await
        .unwrap();
    first.commit().await.unwrap();
    let outcome = update(&mut second, &by_second.value, by_second.version, at(11)).await;

    assert!(
        matches!(outcome, Err(PersistenceError::Conflict { expected }) if expected == Version::INITIAL),
        "{outcome:?}"
    );
    second.rollback().await.unwrap();
    let stored = load(&mut pool.acquire().await.unwrap(), team, expense.id()).await;
    assert_eq!(stored.value.status(), ExpenseStatus::Submitted);
    assert_eq!(stored.version, Version::from_stored(1));
}

#[tokio::test]
async fn another_teams_expense_is_not_found() {
    let database = TestDatabase::new().await.unwrap();
    let mut connection = database.pool().acquire().await.unwrap();
    let (team_a, team_b) = (TeamId::new_v7(), TeamId::new_v7());
    let expense = draft(team_a, claimant());
    insert(&mut connection, &expense, at(10)).await.unwrap();

    assert!(find(&mut connection, team_b, expense.id()).await.unwrap().is_none());
    assert!(
        find(&mut connection, team_a, ExpenseId::new_v7())
            .await
            .unwrap()
            .is_none()
    );
    assert!(find(&mut connection, team_a, expense.id()).await.unwrap().is_some());
}

#[tokio::test]
async fn another_team_can_neither_update_nor_delete_it() {
    let database = TestDatabase::new().await.unwrap();
    let mut connection = database.pool().acquire().await.unwrap();
    let (team_a, team_b) = (TeamId::new_v7(), TeamId::new_v7());
    let expense = draft(team_a, claimant());
    insert(&mut connection, &expense, at(10)).await.unwrap();

    let outcome = delete(&mut connection, team_b, expense.id(), Version::INITIAL).await;
    assert!(matches!(outcome, Err(PersistenceError::Conflict { .. })), "{outcome:?}");
    assert!(find(&mut connection, team_a, expense.id()).await.unwrap().is_some());
}

#[tokio::test]
async fn delete_needs_the_current_version_and_takes_the_rejections_along() {
    let database = TestDatabase::new().await.unwrap();
    let mut connection = database.pool().acquire().await.unwrap();
    let team = TeamId::new_v7();
    let (claimant, approver) = (claimant(), approver());
    let mut expense = draft(team, claimant);
    insert(&mut connection, &expense, at(10)).await.unwrap();
    expense.submit(claimant, ReceiptSummary::new(1, 0)).unwrap();
    expense.reject(approver, "No receipt").unwrap();
    let version = update(&mut connection, &expense, Version::INITIAL, at(11))
        .await
        .unwrap();

    let stale = delete(&mut connection, team, expense.id(), Version::INITIAL).await;
    assert!(matches!(stale, Err(PersistenceError::Conflict { .. })), "{stale:?}");

    delete(&mut connection, team, expense.id(), version).await.unwrap();
    assert!(find(&mut connection, team, expense.id()).await.unwrap().is_none());
    let rejections: i64 = sqlx::query_scalar("select count(*) from expense_rejections where expense_id = $1")
        .bind(expense.id().as_uuid())
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(rejections, 0);
}

#[tokio::test]
async fn a_row_the_domain_could_not_have_written_is_reported_not_loaded() {
    let database = TestDatabase::new().await.unwrap();
    let mut connection = database.pool().acquire().await.unwrap();
    let team = TeamId::new_v7();
    let expense = draft(team, claimant());
    insert(&mut connection, &expense, at(10)).await.unwrap();
    sqlx::query("update expenses set status = 'LOST' where id = $1")
        .bind(expense.id().as_uuid())
        .execute(&mut *connection)
        .await
        .unwrap();

    let outcome = find(&mut connection, team, expense.id()).await;
    let Err(PersistenceError::Corrupt { id, reason }) = outcome else {
        panic!("a row with an unknown status was loaded: {outcome:?}");
    };
    assert_eq!((id, reason.as_str()), (expense.id().as_uuid(), "invalid status"));
}

#[tokio::test]
async fn a_lost_update_is_impossible_even_when_the_second_writer_never_read() {
    // `update` with a version nobody has stored yet, as a client with a made-up ETag would send.
    let database = TestDatabase::new().await.unwrap();
    let mut connection = database.pool().acquire().await.unwrap();
    let team = TeamId::new_v7();
    let expense = draft(team, claimant());
    insert(&mut connection, &expense, at(10)).await.unwrap();

    let outcome = update(&mut connection, &expense, Version::from_stored(5), at(11)).await;
    assert!(matches!(outcome, Err(PersistenceError::Conflict { .. })), "{outcome:?}");
}
