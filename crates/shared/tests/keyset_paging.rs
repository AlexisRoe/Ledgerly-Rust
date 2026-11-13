//! Keyset paging against the `expenses` table (plan M4.2).
//!
//! Java: `src/test/java/com/ledgerly/shared/paging/KeysetPagingTest.java`.

// clippy.toml exempts `#[test]` functions only; the helpers of an integration test are test code too.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::id::TeamId;
use ledgerly_shared::paging::{Cursor, KeysetPage, fetch};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
struct Row {
    id: Uuid,
    created_at: DateTime<Utc>,
}

fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
}

async fn insert(pool: &PgPool, team: TeamId, created_at: DateTime<Utc>) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into expenses (id, team_id, claimant_id, amount, currency, category, incurred_on, merchant, \
                               status, version, created_at, updated_at) \
         values ($1, $2, gen_random_uuid(), 1.00, 'EUR', 'MEALS', date '2026-01-01', 'Café', 'DRAFT', 0, $3, $3)",
    )
    .bind(id)
    .bind(team.as_uuid())
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
    id
}

/// Java: `seed`. Pairs of rows share a timestamp, so the id tie-breaker is exercised.
async fn seed(pool: &PgPool, team: TeamId, count: i64) -> BTreeSet<Uuid> {
    let mut ids = BTreeSet::new();
    for index in 0..count {
        ids.insert(insert(pool, team, start() + TimeDelta::seconds(index / 2)).await);
    }
    ids
}

async fn page(pool: &PgPool, team: TeamId, after: Option<Cursor>, limit: u32) -> KeysetPage<Row> {
    let mut connection = pool.acquire().await.unwrap();
    fetch(&mut connection, "expenses", team, after, limit, |row: &Row| {
        Cursor::new(row.created_at, row.id)
    })
    .await
    .unwrap()
}

/// Every page until there is no cursor, the cursor going through its text form as it does through a client.
async fn page_through(pool: &PgPool, team: TeamId, mut after: Option<Cursor>, seen: &mut Vec<Row>) -> usize {
    let mut pages = 0;
    loop {
        let current = page(pool, team, after, 100).await;
        seen.extend(current.items);
        pages += 1;
        match current.next {
            Some(next) => after = Some(Cursor::decode(&next.encode()).unwrap()),
            None => return pages,
        }
    }
}

fn ids(rows: &[Row]) -> Vec<Uuid> {
    rows.iter().map(|row| row.id).collect()
}

fn without_duplicates(rows: &[Row]) -> BTreeSet<Uuid> {
    let unique: BTreeSet<Uuid> = ids(rows).into_iter().collect();
    assert_eq!(unique.len(), rows.len(), "a row was returned twice");
    unique
}

/// Java: `pagesThroughEveryRowExactlyOnceNewestFirst`.
#[tokio::test]
async fn pages_through_every_row_exactly_once_newest_first() {
    let database = TestDatabase::new().await.unwrap();
    let team = TeamId::new_v7();
    let expected = seed(database.pool(), team, 250).await;

    let mut seen = Vec::new();
    let pages = page_through(database.pool(), team, None, &mut seen).await;

    assert_eq!(pages, 3);
    assert_eq!(without_duplicates(&seen), expected);
    assert!(
        seen.windows(2)
            .all(|pair| (pair[0].created_at, pair[0].id) > (pair[1].created_at, pair[1].id))
    );
}

/// Java: `lastPageHasNoCursor`.
#[tokio::test]
async fn the_last_page_has_no_cursor() {
    let database = TestDatabase::new().await.unwrap();
    let team = TeamId::new_v7();
    seed(database.pool(), team, 5).await;

    let first = page(database.pool(), team, None, 5).await;

    assert_eq!(first.items.len(), 5);
    assert_eq!(first.next, None);
}

/// One row more than the page: the cursor points at the last row returned.
#[tokio::test]
async fn a_full_page_with_more_to_come_has_the_cursor_of_its_last_row() {
    let database = TestDatabase::new().await.unwrap();
    let team = TeamId::new_v7();
    seed(database.pool(), team, 6).await;

    let first = page(database.pool(), team, None, 5).await;

    let last = first.items.last().unwrap();
    assert_eq!(first.next, Some(Cursor::new(last.created_at, last.id)));
    assert_eq!(page(database.pool(), team, first.next, 5).await.items.len(), 1);
}

/// Java: `emptyTeamGivesEmptyPage`.
#[tokio::test]
async fn an_empty_team_gives_an_empty_page() {
    let database = TestDatabase::new().await.unwrap();

    let first = page(database.pool(), TeamId::new_v7(), None, 10).await;

    assert_eq!(
        first,
        KeysetPage {
            items: Vec::new(),
            next: None
        }
    );
}

/// Java: `rowsInsertedWhilePagingDoNotCauseDuplicatesOrLoss`.
#[tokio::test]
async fn rows_inserted_while_paging_cause_no_duplicates_or_loss() {
    let database = TestDatabase::new().await.unwrap();
    let team = TeamId::new_v7();
    let original = seed(database.pool(), team, 250).await;

    let first = page(database.pool(), team, None, 100).await;
    // New rows arrive, newer than everything, after the client fetched page one.
    for index in 0..20 {
        insert(database.pool(), team, start() + TimeDelta::seconds(10_000 + index)).await;
    }
    let mut seen = first.items;
    page_through(database.pool(), team, first.next, &mut seen).await;

    assert_eq!(without_duplicates(&seen), original);
}

/// Java: `otherTeamsRowsAreNeverReturned`.
#[tokio::test]
async fn other_teams_rows_are_never_returned() {
    let database = TestDatabase::new().await.unwrap();
    let team = TeamId::new_v7();
    seed(database.pool(), team, 3).await;
    insert(database.pool(), TeamId::new_v7(), start()).await;

    assert_eq!(page(database.pool(), team, None, 50).await.items.len(), 3);
}
