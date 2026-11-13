//! `CreateExpense` and `GetExpense` against real Postgres (plan M2.4).
//!
//! In-crate, because the use cases are `pub(crate)` until the web layer (M3) serves them (ADR 0010).

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::money::Money;
use ledgerly_shared::version::Version;
use sqlx::PgPool;

use super::*;
use crate::domain::{Category, ExpenseStatus, InvalidInput, NewExpense};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, 0).unwrap()
}

fn clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock::at(now()))
}

fn lunch() -> NewExpense {
    NewExpense {
        money: Money::parse("12.5", "EUR").unwrap(),
        category: Category::Meals,
        incurred_on: NaiveDate::from_ymd_opt(2026, 11, 5).unwrap(),
        merchant: "  Café Müller ".to_owned(),
        note: Some("Team lunch".to_owned()),
    }
}

fn command(team: TeamId, expense: NewExpense) -> CreateExpenseCommand {
    CreateExpenseCommand {
        team,
        claimant: UserId::new_v7(),
        expense,
    }
}

async fn stored_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("select count(*) from expenses")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The first rule the use case refused with, or a panic if it did not refuse with a field rule.
async fn refusal(pool: &PgPool, expense: NewExpense) -> InvalidInput {
    let outcome = CreateExpense::new(pool.clone(), clock())
        .execute(command(TeamId::new_v7(), expense))
        .await;
    match outcome {
        Err(UseCaseError::Refused(ExpenseError::InvalidInput(invalid))) => invalid,
        other => panic!("expected a field rule to refuse, got {other:?}"),
    }
}

#[tokio::test]
async fn create_stores_a_draft_for_the_caller_at_version_0_and_get_reads_it_back() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool().clone();
    let team = TeamId::new_v7();
    let command = command(team, lunch());
    let claimant = command.claimant;

    let created = CreateExpense::new(pool.clone(), clock())
        .execute(command)
        .await
        .unwrap();
    let read = GetExpense::new(pool.clone())
        .execute(team, created.value.id())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(created.version, Version::INITIAL);
    assert_eq!(read.version, Version::INITIAL);
    let expense = &read.value;
    assert_eq!(expense.id(), created.value.id());
    assert_eq!(expense.team_id(), team);
    assert_eq!(expense.claimant_id(), claimant);
    assert_eq!(expense.status(), ExpenseStatus::Draft);
    assert_eq!(expense.money(), Money::parse("12.50", "EUR").unwrap());
    assert_eq!(expense.category(), Category::Meals);
    assert_eq!(expense.incurred_on(), NaiveDate::from_ymd_opt(2026, 11, 5).unwrap());
    assert_eq!(
        expense.merchant().as_str(),
        "Café Müller",
        "the merchant is stored stripped"
    );
    assert_eq!(expense.note().map(|note| note.as_str()), Some("Team lunch"));
    assert!(expense.rejections().is_empty());

    let (created_at, updated_at): (DateTime<Utc>, DateTime<Utc>) =
        sqlx::query_as("select created_at, updated_at from expenses where id = $1")
            .bind(expense.id().as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (created_at, updated_at),
        (now(), now()),
        "the time comes from the clock"
    );
}

#[tokio::test]
async fn get_does_not_find_another_teams_expense_or_a_missing_one() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool().clone();
    let team = TeamId::new_v7();
    let created = CreateExpense::new(pool.clone(), clock())
        .execute(command(team, lunch()))
        .await
        .unwrap();
    let get = GetExpense::new(pool);

    assert!(
        get.execute(TeamId::new_v7(), created.value.id())
            .await
            .unwrap()
            .is_none()
    );
    assert!(get.execute(team, ExpenseId::new_v7()).await.unwrap().is_none());
}

#[tokio::test]
async fn a_blank_merchant_is_refused_and_nothing_is_written() {
    let database = TestDatabase::new().await.unwrap();
    let blank = NewExpense {
        merchant: " \u{a0} ".to_owned(),
        ..lunch()
    };
    // U+00A0 is not whitespace to Java's `String.strip`, so that merchant is not blank. A tab is.
    let tab = NewExpense {
        merchant: "\t".to_owned(),
        ..lunch()
    };

    assert!(
        CreateExpense::new(database.pool().clone(), clock())
            .execute(command(TeamId::new_v7(), blank))
            .await
            .is_ok()
    );
    let refused = refusal(database.pool(), tab).await;

    assert_eq!(
        (refused.field, refused.message.as_str()),
        ("merchant", "Merchant is required")
    );
    assert_eq!(stored_rows(database.pool()).await, 1);
}

#[tokio::test]
async fn a_future_date_is_refused() {
    let database = TestDatabase::new().await.unwrap();
    let tomorrow = NewExpense {
        incurred_on: NaiveDate::from_ymd_opt(2026, 11, 6).unwrap(),
        ..lunch()
    };

    let refused = refusal(database.pool(), tomorrow).await;

    assert_eq!(
        (refused.field, refused.message.as_str()),
        ("incurredOn", "incurredOn must not be in the future")
    );
    assert_eq!(stored_rows(database.pool()).await, 0);
}

#[tokio::test]
async fn a_note_over_2000_characters_is_refused() {
    let database = TestDatabase::new().await.unwrap();
    let long = NewExpense {
        note: Some("x".repeat(2001)),
        ..lunch()
    };

    let refused = refusal(database.pool(), long).await;

    assert_eq!(refused.field, "note");
    assert_eq!(stored_rows(database.pool()).await, 0);
}

#[tokio::test]
async fn the_first_broken_field_is_reported_in_javas_order() {
    let database = TestDatabase::new().await.unwrap();
    let everything_wrong = NewExpense {
        merchant: String::new(),
        note: Some("x".repeat(2001)),
        incurred_on: NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(),
        ..lunch()
    };

    assert_eq!(refusal(database.pool(), everything_wrong).await.field, "merchant");
}

#[tokio::test]
async fn invalid_money_cannot_even_reach_the_use_case() {
    // The command holds a `Money`, so `12.345 EUR` or `-1 EUR` is refused where the text is parsed (the HTTP
    // edge from M3), with the messages of the M0 constructor.
    assert_eq!(
        Money::parse("12.345", "EUR").unwrap_err().to_string(),
        "Amount 12.345 has more than 2 decimal places for EUR"
    );
    assert_eq!(
        Money::parse("-1", "EUR").unwrap_err().to_string(),
        "Amount must be positive: -1"
    );
}
