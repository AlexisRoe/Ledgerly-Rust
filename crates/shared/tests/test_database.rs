//! The test database harness (plan M2.2): every test gets a migrated database nobody else writes to.
//!
//! The two `same_row_in_*` tests insert a row with the same primary key and expect to be alone with it. Under
//! nextest they run in parallel processes, under `cargo test` in parallel threads. Sharing a database in any
//! order would fail one of them, on the duplicate key or on the count.

// clippy.toml exempts `#[test]` functions only; the helpers of an integration test are test code too.
#![allow(clippy::unwrap_used)]

use ledgerly_shared::database::{TestDatabase, verify};
use sqlx::PgPool;

const SHARED_ID: &str = "0192f3a0-0000-7000-8000-000000000001";

async fn insert_the_shared_expense(pool: &PgPool) {
    sqlx::query(
        "insert into expenses (id, team_id, claimant_id, amount, currency, category, incurred_on, merchant, \
                               status, version, created_at, updated_at) \
         values ($1::uuid, gen_random_uuid(), gen_random_uuid(), 12.50, 'EUR', 'MEALS', date '2026-11-01', \
                 'Café', 'DRAFT', 0, now(), now())",
    )
    .bind(SHARED_ID)
    .execute(pool)
    .await
    .unwrap();
}

async fn expense_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("select count(*) from expenses")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn same_row_in_one_test() {
    let database = TestDatabase::new().await.unwrap();
    insert_the_shared_expense(database.pool()).await;
    assert_eq!(expense_count(database.pool()).await, 1);
}

#[tokio::test]
async fn same_row_in_another_test() {
    let database = TestDatabase::new().await.unwrap();
    insert_the_shared_expense(database.pool()).await;
    assert_eq!(expense_count(database.pool()).await, 1);
}

#[tokio::test]
async fn two_databases_of_one_test_are_separate_too() {
    let (first, second) = tokio::join!(TestDatabase::new(), TestDatabase::new());
    let (first, second) = (first.unwrap(), second.unwrap());
    insert_the_shared_expense(first.pool()).await;

    assert_eq!(expense_count(first.pool()).await, 1);
    assert_eq!(expense_count(second.pool()).await, 0);
}

#[tokio::test]
async fn a_new_database_is_migrated_and_empty() {
    let database = TestDatabase::new().await.unwrap();
    verify(database.pool()).await.unwrap();
    assert_eq!(expense_count(database.pool()).await, 0);
}

#[tokio::test]
async fn committed_work_stays_in_the_tests_own_database() {
    // Transactions commit for real; isolation comes from the database, not from a rollback.
    let database = TestDatabase::new().await.unwrap();
    let mut transaction = database.pool().begin().await.unwrap();
    sqlx::query("update expenses set merchant = 'nobody' where false")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    insert_the_shared_expense(database.pool()).await;

    let reopened = PgPool::connect_with(ledgerly_shared::database::connect_options(database.config()))
        .await
        .unwrap();
    assert_eq!(expense_count(&reopened).await, 1);
}
