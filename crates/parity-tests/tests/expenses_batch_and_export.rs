//! Creating many drafts in one request, and exporting the list as CSV.
//!
//! Java: `src/test/java/com/ledgerly/expenses/internal/web/BatchAndExportApiTest.java`, which caps exports at
//! 1,200 rows. This file runs in the `export` group with the same setting. Its `member` and `other` are the
//! team's MEMBER and colleague here.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeSet;

use chrono::{Days, NaiveDate, Utc};
use ledgerly_parity_tests::fixtures::Expense;
use ledgerly_parity_tests::http::{Client, Response};
use ledgerly_parity_tests::{SignedInTeam, World, today};
use serde_json::{Value, json};
use uuid::Uuid;

/// Java: `item()`.
fn item(amount: &str, currency: &str, date: &str, merchant: &str) -> Value {
    json!({
        "money": {"amount": amount, "currency": currency},
        "category": "MEALS",
        "incurredOn": date,
        "merchant": merchant,
    })
}

fn items(items: Vec<Value>) -> Value {
    json!({ "items": items })
}

/// Java: `good(n)`.
fn good(n: usize) -> Value {
    item("1.00", "EUR", &today(), &format!("Merchant {n}"))
}

fn days_from_now(days: u64) -> String {
    Utc::now()
        .date_naive()
        .checked_add_days(Days::new(days))
        .unwrap()
        .to_string()
}

fn batch_path(team: &SignedInTeam, atomic: bool) -> String {
    format!(
        "{}/expenses:batch{}",
        team.path(),
        if atomic { "?atomic=true" } else { "" }
    )
}

async fn batch(client: &Client, team: &SignedInTeam, atomic: bool, body: &Value) -> Response {
    client.post(&batch_path(team, atomic)).json(body).send().await
}

/// Java: `expenseCount()`.
async fn expense_count(world: &World, team: &SignedInTeam) -> i64 {
    sqlx::query_scalar("select count(*) from expenses where team_id = $1")
        .bind(team.id)
        .fetch_one(world.fixtures.pool())
        .await
        .unwrap()
}

/// Java: `seed()`: a 10.00 MEALS expense of `claimant` with this merchant, incurred `offset % 300` days after
/// 2026-01-01.
async fn seed(
    world: &World,
    team: &SignedInTeam,
    claimant: Uuid,
    merchant: &str,
    status: &str,
    currency: &str,
    offset: u64,
) -> Uuid {
    world
        .fixtures
        .insert_expense(&Expense {
            currency: currency.to_owned(),
            merchant: merchant.to_owned(),
            incurred_on: jan_first().checked_add_days(Days::new(offset % 300)).unwrap(),
            ..Expense::new(team.id, claimant, status)
        })
        .await
}

fn jan_first() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
}

async fn export(client: &Client, team: &SignedInTeam, query: &str) -> Response {
    client.get(&format!("{}/expenses.csv{query}", team.path())).send().await
}

/// Java: `lines()`: rows end in CRLF.
fn lines(csv: &str) -> Vec<&str> {
    csv.split("\r\n").filter(|line| !line.is_empty()).collect()
}

mod m8 {
    use super::*;

    // --- batch: partial success (default) ---

    /// Java: `BatchAndExportApiTest.aFullBatchOfHundredCreatesHundredDraftsForTheCaller`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_full_batch_of_hundred_creates_hundred_drafts_for_the_caller() {
        let world = World::start().await;
        let team = world.team().await;

        let response = batch(
            &world.as_user(&team.member),
            &team,
            false,
            &items((0..100).map(good).collect()),
        )
        .await;

        assert_eq!(response.status, 200);
        let body = response.json();
        assert_eq!(body["created"], 100);
        assert_eq!(body["rejected"], 0);
        let results = body["results"].as_array().unwrap();
        assert!(results.iter().all(|result| result["status"] == "CREATED"));
        let ids: BTreeSet<&str> = results.iter().map(|result| result["id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), 100);
        let drafts: i64 = sqlx::query_scalar(
            "select count(*) from expenses where team_id = $1 and status = 'DRAFT' and claimant_id = $2",
        )
        .bind(team.id)
        .bind(team.member.id())
        .fetch_one(world.fixtures.pool())
        .await
        .unwrap();
        assert_eq!(drafts, 100);
    }

    /// Java: `BatchAndExportApiTest.moreThanHundredItemsGets400AndCreatesNothing`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn more_than_hundred_items_gets_400_and_creates_nothing() {
        let world = World::start().await;
        let team = world.team().await;

        let response = batch(
            &world.as_user(&team.member),
            &team,
            false,
            &items((0..101).map(good).collect()),
        )
        .await;

        assert_eq!(response.status, 400);
        assert_eq!(expense_count(&world, &team).await, 0);
    }

    /// Java: `BatchAndExportApiTest.anEmptyBatchGets400`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn an_empty_batch_gets_400() {
        let world = World::start().await;
        let team = world.team().await;

        assert_eq!(
            batch(&world.as_user(&team.member), &team, false, &items(vec![]))
                .await
                .status,
            400
        );
    }

    /// Java: `BatchAndExportApiTest.invalidItemsAreRejectedIndividuallyAndTheRestAreCreated`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn invalid_items_are_rejected_individually_and_the_rest_are_created() {
        let world = World::start().await;
        let team = world.team().await;
        let body = items(vec![
            good(0),
            item("12.345", "EUR", &today(), "too many decimals"),
            good(2),
            item("1.00", "EUR", &days_from_now(3), "future"),
            item("1.00", "EUR", &today(), "   "),
            item("abc", "EUR", &today(), "not a number"),
            item("1.00", "ZZZ", &today(), "no such currency"),
            good(7),
        ]);

        let response = batch(&world.as_user(&team.member), &team, false, &body).await;

        assert_eq!(response.status, 200);
        let body = response.json();
        assert_eq!(body["created"], 3);
        assert_eq!(body["rejected"], 5);
        let results = body["results"].as_array().unwrap();
        let statuses: Vec<&str> = results
            .iter()
            .map(|result| result["status"].as_str().unwrap())
            .collect();
        assert_eq!(
            statuses,
            [
                "CREATED", "REJECTED", "CREATED", "REJECTED", "REJECTED", "REJECTED", "REJECTED", "CREATED"
            ]
        );
        for (index, result) in results.iter().enumerate() {
            assert_eq!(result["index"], index);
        }
        for (index, field) in [
            (1, "money"),
            (3, "incurredOn"),
            (4, "merchant"),
            (5, "money"),
            (6, "money"),
        ] {
            assert_eq!(results[index]["problem"]["field"], field, "item {index}");
        }
        assert!(results[1].get("id").is_none_or(Value::is_null));
        assert_eq!(expense_count(&world, &team).await, 3);
    }

    // --- batch: atomic ---

    /// Java: `BatchAndExportApiTest.anAtomicBatchWithOneBadItemCreatesNothing`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn an_atomic_batch_with_one_bad_item_creates_nothing() {
        let world = World::start().await;
        let team = world.team().await;
        let body = items(vec![
            good(0),
            good(1),
            item("1.00", "EUR", &days_from_now(1), "future"),
            good(3),
        ]);

        let response = batch(&world.as_user(&team.member), &team, true, &body).await;

        assert_eq!(response.status, 422);
        assert_eq!(response.problem_code().as_deref(), Some("BATCH_REJECTED"));
        let errors = response.json()["errors"].clone();
        assert_eq!(errors.as_array().unwrap().len(), 1);
        assert_eq!(errors[0]["field"], "items[2].incurredOn");
        assert_eq!(expense_count(&world, &team).await, 0);
    }

    /// Java: `BatchAndExportApiTest.anAtomicBatchListsEveryProblem`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn an_atomic_batch_lists_every_problem() {
        let world = World::start().await;
        let team = world.team().await;
        let body = items(vec![
            item("x", "EUR", &today(), "a"),
            good(1),
            item("1.00", "EUR", &today(), " "),
        ]);

        let response = batch(&world.as_user(&team.member), &team, true, &body).await;

        assert_eq!(response.status, 422);
        let fields: Vec<String> = response.json()["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| error["field"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(fields, ["items[0].money", "items[2].merchant"]);
        assert_eq!(expense_count(&world, &team).await, 0);
    }

    /// Java: `BatchAndExportApiTest.anAtomicBatchOfValidItemsCreatesAll`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn an_atomic_batch_of_valid_items_creates_all() {
        let world = World::start().await;
        let team = world.team().await;

        let response = batch(
            &world.as_user(&team.member),
            &team,
            true,
            &items(vec![good(0), good(1), good(2)]),
        )
        .await;

        assert_eq!(response.status, 200);
        assert_eq!(response.json()["created"], 3);
        assert_eq!(expense_count(&world, &team).await, 3);
    }

    // --- batch: abuse ---

    /// Java: `BatchAndExportApiTest.anItemCannotSetStatusOrClaimant_API3`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn an_item_cannot_set_status_or_claimant() {
        let world = World::start().await;
        let team = world.team().await;
        let mut sneaky = item("1.00", "EUR", &today(), "x");
        sneaky["status"] = json!("APPROVED");
        sneaky["claimantId"] = json!(team.colleague.id());

        let response = batch(&world.as_user(&team.member), &team, false, &items(vec![sneaky])).await;

        assert_eq!(response.status, 400);
        assert_eq!(expense_count(&world, &team).await, 0);
    }

    /// Java: `BatchAndExportApiTest.aNonTeammateGets404AndAnonymousGets401`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_non_teammate_gets_404_and_anonymous_gets_401() {
        let world = World::start().await;
        let team = world.team().await;
        let outsider = world.as_user(&world.user().await);

        assert_eq!(batch(&outsider, &team, false, &items(vec![good(0)])).await.status, 404);
        assert_eq!(
            batch(&world.anonymous(), &team, false, &items(vec![good(0)]))
                .await
                .status,
            401
        );
        assert_eq!(expense_count(&world, &team).await, 0);
    }

    /// Java: `BatchAndExportApiTest.aHugeJsonBodyIsRefusedWith413BeforeItIsParsed_API4`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_huge_json_body_is_refused_with_413_before_it_is_parsed() {
        let world = World::start().await;
        let team = world.team().await;
        let padding = "x".repeat(2 * 1024 * 1024);

        let response = batch(
            &world.as_user(&team.member),
            &team,
            false,
            &items(vec![item("1.00", "EUR", &today(), &padding)]),
        )
        .await;

        assert_eq!(response.status, 413);
        assert_eq!(response.problem_code().as_deref(), Some("PAYLOAD_TOO_LARGE"));
        assert_eq!(expense_count(&world, &team).await, 0);
    }

    /// Java: `BatchAndExportApiTest.theBodyLimitAppliesBeforeAuthenticationToo`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn the_body_limit_applies_before_authentication_too() {
        let world = World::start().await;
        let team = world.team().await;
        let huge = format!("{{\"pad\":\"{}\"}}", "x".repeat(2 * 1024 * 1024));

        let response = world
            .anonymous()
            .post(&batch_path(&team, false))
            .body("application/json", huge)
            .send()
            .await;

        assert_eq!(response.status, 413);
    }

    // --- export ---

    /// Java: `BatchAndExportApiTest.exportReturnsAnAttachmentWithHeaderAndRows`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn export_returns_an_attachment_with_header_and_rows() {
        let world = World::start().await;
        let team = world.team().await;
        seed(&world, &team, team.member.id(), "Cafe Central", "DRAFT", "EUR", 1).await;

        let response = export(&world.as_user(&team.approver), &team, "").await;

        assert_eq!(response.status, 200);
        assert!(response.header("content-type").unwrap().starts_with("text/csv"));
        let disposition = response.header("content-disposition").unwrap();
        assert!(
            disposition.contains("attachment") && disposition.contains("expenses.csv"),
            "{disposition}"
        );
        assert_eq!(response.header("x-total-count"), Some("1"));
        assert_eq!(response.header("x-truncated"), Some("false"));
        let text = response.text();
        let lines = lines(&text);
        assert_eq!(
            lines[0],
            "id,incurredOn,merchant,amount,currency,category,status,claimantId,note"
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("Cafe Central"));
        assert!(
            lines[1].contains(&format!(",10.00,EUR,MEALS,DRAFT,{}", team.member.id())),
            "{}",
            lines[1]
        );
    }

    /// Java: `BatchAndExportApiTest.formulaCellsAreNeutralisedAndSpecialCharactersAreQuoted_API10`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn formula_cells_are_neutralised_and_special_characters_are_quoted() {
        let world = World::start().await;
        let team = world.team().await;
        let member = team.member.id();
        for (offset, merchant) in [
            r#"=HYPERLINK("http://evil.test","click")"#,
            "+1+1",
            "-2+3",
            "@SUM(A1)",
            "\tTabbed",
            "Smith, Jones & Co",
        ]
        .into_iter()
        .enumerate()
        {
            seed(&world, &team, member, merchant, "DRAFT", "EUR", offset as u64 + 1).await;
        }

        let csv = export(&world.as_user(&team.approver), &team, "").await.text();

        // No data line may start a cell with a formula character.
        assert!(csv.contains(r#"'=HYPERLINK(""http://evil.test"",""click"")"#), "{csv}");
        for neutralised in [",'+1+1,", ",'-2+3,", ",'@SUM(A1),", ",'\tTabbed,"] {
            assert!(csv.contains(neutralised), "{neutralised:?} in {csv}");
        }
        assert!(csv.contains("\"Smith, Jones & Co\""), "{csv}");
        for line in lines(&csv).into_iter().skip(1) {
            for dangerous in [",=", ",+", ",-", ",@", ",\t"] {
                assert!(!line.contains(dangerous), "{line}");
            }
        }
    }

    /// Java: `BatchAndExportApiTest.aMemberExportsOnlyTheirOwnExpenses_API1`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_member_exports_only_their_own_expenses() {
        let world = World::start().await;
        let team = world.team().await;
        seed(&world, &team, team.member.id(), "mine", "DRAFT", "EUR", 1).await;
        seed(&world, &team, team.colleague.id(), "theirs", "DRAFT", "EUR", 2).await;

        let mine = export(
            &world.as_user(&team.member),
            &team,
            &format!("?claimantId={}", team.colleague.id()),
        )
        .await;
        let all = export(&world.as_user(&team.approver), &team, "").await;

        assert!(mine.text().contains("mine") && !mine.text().contains("theirs"));
        assert!(all.text().contains("mine") && all.text().contains("theirs"));
    }

    /// Java: `BatchAndExportApiTest.exportUsesTheSameFiltersAsTheList`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn export_uses_the_same_filters_as_the_list() {
        let world = World::start().await;
        let team = world.team().await;
        let member = team.member.id();
        seed(&world, &team, member, "draft-eur", "DRAFT", "EUR", 1).await;
        seed(&world, &team, member, "sub-eur", "SUBMITTED", "EUR", 2).await;
        seed(&world, &team, member, "sub-usd", "SUBMITTED", "USD", 3).await;

        let csv = export(&world.as_user(&team.approver), &team, "?status=SUBMITTED&currency=EUR")
            .await
            .text();

        assert!(csv.contains("sub-eur"));
        assert!(!csv.contains("draft-eur") && !csv.contains("sub-usd"));
    }

    /// Java: `BatchAndExportApiTest.exportStreamsManyPagesWithoutRepeatsOrGaps`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn export_streams_many_pages_without_repeats_or_gaps() {
        let world = World::start().await;
        let team = world.team().await;
        let now = Utc::now();
        let mut expected = BTreeSet::new();
        for i in 0..1150_u64 {
            let id = world
                .fixtures
                .insert_expense(&Expense {
                    amount: "1.00".parse().unwrap(),
                    incurred_on: jan_first().checked_add_days(Days::new(i % 200)).unwrap(),
                    created_at: now,
                    ..Expense::new(team.id, team.member.id(), "DRAFT")
                })
                .await;
            expected.insert(id.to_string());
        }

        let response = export(&world.as_user(&team.approver), &team, "").await;

        assert_eq!(response.header("x-total-count"), Some("1150"));
        assert_eq!(response.header("x-truncated"), Some("false"));
        let text = response.text();
        let lines = lines(&text);
        assert_eq!(lines.len(), 1151);
        let seen: BTreeSet<String> = lines[1..].iter().map(|line| line[..36].to_owned()).collect();
        assert_eq!(seen, expected);
    }

    /// Java: `BatchAndExportApiTest.exportIsCappedAndSaysSo`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn export_is_capped_and_says_so() {
        let world = World::start().await;
        let team = world.team().await;
        for _ in 0..1300 {
            world
                .fixtures
                .insert_expense(&Expense {
                    amount: "1.00".parse().unwrap(),
                    incurred_on: jan_first(),
                    ..Expense::new(team.id, team.member.id(), "DRAFT")
                })
                .await;
        }

        let response = export(&world.as_user(&team.approver), &team, "").await;

        assert_eq!(response.header("x-total-count"), Some("1300"));
        assert_eq!(response.header("x-truncated"), Some("true"));
        // The header and the 1,200 rows this group's cap allows.
        assert_eq!(lines(&response.text()).len(), 1201);
    }

    /// Java: `BatchAndExportApiTest.exportOfNothingIsJustTheHeader`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn export_of_nothing_is_just_the_header() {
        let world = World::start().await;
        let team = world.team().await;

        assert_eq!(
            lines(&export(&world.as_user(&team.approver), &team, "").await.text()).len(),
            1
        );
    }

    /// Java: `BatchAndExportApiTest.exportAuthorizationAndValidation`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn export_authorization_and_validation() {
        let world = World::start().await;
        let team = world.team().await;
        let approver = world.as_user(&team.approver);

        assert_eq!(export(&world.as_user(&world.user().await), &team, "").await.status, 404);
        assert_eq!(export(&world.anonymous(), &team, "").await.status, 401);
        for query in [
            "?currency=ZZZ",
            "?currency=eur",
            "?status=BOGUS",
            "?incurredFrom=2026-02-01&incurredTo=2026-01-01",
        ] {
            assert_eq!(export(&approver, &team, query).await.status, 400, "{query}");
        }
    }
}
