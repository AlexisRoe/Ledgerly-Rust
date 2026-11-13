//! Listing expenses: paging, sorting, filters and who sees what.
//!
//! Java: `src/test/java/com/ledgerly/expenses/internal/web/ExpensesListApiTest.java`. Its `alice` and `bob`
//! are the team's MEMBER and colleague here; its `approver` is the APPROVER (and the OWNER where Java gives
//! the same user that role).

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use ledgerly_parity_tests::fixtures::Expense;
use ledgerly_parity_tests::http::Response;
use ledgerly_parity_tests::{Session, SignedInTeam, World};
use rust_decimal::Decimal;
use uuid::Uuid;

/// Java: `t0`: the creation time of the first seeded expense.
fn t0() -> DateTime<Utc> {
    "2026-02-01T00:00:00Z".parse().unwrap()
}

/// Java: `seed()`: an expense with the given columns, created `offset` seconds after `t0`.
async fn seed(
    world: &World,
    team: &SignedInTeam,
    claimant: &Session,
    (amount, currency, category, status, date): (&str, &str, &str, &str, &str),
    offset: i64,
) -> Uuid {
    world
        .fixtures
        .insert_expense(&Expense {
            amount: amount.parse::<Decimal>().unwrap(),
            currency: currency.to_owned(),
            category: category.to_owned(),
            incurred_on: date.parse::<NaiveDate>().unwrap(),
            created_at: t0() + Duration::seconds(offset),
            ..Expense::new(team.id, claimant.id(), status)
        })
        .await
}

/// A draft of 1.00 EUR, MEALS, incurred on `date`.
async fn draft(world: &World, team: &SignedInTeam, claimant: &Session, date: &str, offset: i64) -> Uuid {
    seed(world, team, claimant, ("1.00", "EUR", "MEALS", "DRAFT", date), offset).await
}

async fn list(world: &World, team: &SignedInTeam, user: &Session, query: &str) -> Response {
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    world
        .as_user(user)
        .get(&format!("{}/expenses{query}", team.path()))
        .send()
        .await
}

fn ids(response: &Response) -> Vec<String> {
    response.json()["content"]
        .as_array()
        .unwrap_or_else(|| panic!("no content in {}", response.text()))
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

fn strings(ids: &[Uuid]) -> Vec<String> {
    ids.iter().map(Uuid::to_string).collect()
}

mod m4 {
    use super::*;

    // --- paging ---

    /// Java: `ExpensesListApiTest.pagingThrough250ExpensesVisitsEachOnce`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn paging_through_250_expenses_visits_each_once() {
        let world = World::start().await;
        let team = world.team().await;
        let mut expected = BTreeSet::new();
        for offset in 0..250 {
            expected.insert(
                draft(&world, &team, &team.member, "2026-01-15", offset)
                    .await
                    .to_string(),
            );
        }

        let mut seen = Vec::new();
        for page in 0..3 {
            let response = list(&world, &team, &team.approver, &format!("size=100&page={page}")).await;
            assert_eq!(response.status, 200);
            let meta = &response.json()["page"];
            assert_eq!(meta["totalElements"], 250);
            assert_eq!(meta["totalPages"], 3);
            assert_eq!(meta["size"], 100);
            assert_eq!(meta["number"], page);
            seen.extend(ids(&response));
        }

        assert_eq!(seen.len(), 250);
        assert_eq!(seen.iter().cloned().collect::<BTreeSet<_>>(), expected);
    }

    /// Java: `ExpensesListApiTest.pageBeyondTheEndIsEmptyNotAnError`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn page_beyond_the_end_is_empty_not_an_error() {
        let world = World::start().await;
        let team = world.team().await;
        draft(&world, &team, &team.member, "2026-01-15", 0).await;

        let response = list(&world, &team, &team.approver, "page=5").await;

        assert_eq!(response.status, 200);
        assert!(ids(&response).is_empty());
    }

    /// Java: `ExpensesListApiTest.sizeAbove100IsRejectedNotClamped_API4`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn size_above_100_is_rejected_not_clamped() {
        let world = World::start().await;
        let team = world.team().await;

        let response = list(&world, &team, &team.approver, "size=101").await;

        assert_eq!(response.status, 400);
        assert_eq!(response.problem_code().as_deref(), Some("VALIDATION_FAILED"));
    }

    /// Java: `ExpensesListApiTest.zeroNegativeAndNonNumericPagingParametersAreRejected`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn zero_negative_and_non_numeric_paging_parameters_are_rejected() {
        let world = World::start().await;
        let team = world.team().await;

        for query in ["size=0", "page=-1", "size=abc"] {
            assert_eq!(list(&world, &team, &team.approver, query).await.status, 400, "{query}");
        }
    }

    // --- sorting ---

    /// Java: `ExpensesListApiTest.defaultSortIsIncurredOnDescending`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn default_sort_is_incurred_on_descending() {
        let world = World::start().await;
        let team = world.team().await;
        let old = draft(&world, &team, &team.member, "2026-01-01", 0).await;
        let recent = draft(&world, &team, &team.member, "2026-01-20", 1).await;

        assert_eq!(
            ids(&list(&world, &team, &team.approver, "").await),
            strings(&[recent, old])
        );
    }

    /// Java: `ExpensesListApiTest.sortAscendingAndByCreatedAt`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn sort_ascending_and_by_created_at() {
        let world = World::start().await;
        let team = world.team().await;
        let first = draft(&world, &team, &team.member, "2026-01-20", 0).await;
        let second = draft(&world, &team, &team.member, "2026-01-01", 1).await;

        assert_eq!(
            ids(&list(&world, &team, &team.approver, "sort=incurredOn,asc").await),
            strings(&[second, first])
        );
        assert_eq!(
            ids(&list(&world, &team, &team.approver, "sort=createdAt,desc").await),
            strings(&[second, first])
        );
    }

    /// Java: `ExpensesListApiTest.sortByAmountNeedsACurrencyFilter`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn sort_by_amount_needs_a_currency_filter() {
        let world = World::start().await;
        let team = world.team().await;
        seed(
            &world,
            &team,
            &team.member,
            ("5.00", "EUR", "MEALS", "DRAFT", "2026-01-01"),
            0,
        )
        .await;

        let response = list(&world, &team, &team.approver, "sort=amount,asc").await;

        assert_eq!(response.status, 400);
        assert_eq!(response.json()["errors"][0]["field"], "sort");
    }

    /// Java: `ExpensesListApiTest.sortByAmountWithCurrencyOrdersNumerically`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn sort_by_amount_with_currency_orders_numerically() {
        let world = World::start().await;
        let team = world.team().await;
        let m = &team.member;
        let ten = seed(&world, &team, m, ("10.00", "EUR", "MEALS", "DRAFT", "2026-01-01"), 0).await;
        let nine = seed(&world, &team, m, ("9.00", "EUR", "MEALS", "DRAFT", "2026-01-02"), 1).await;
        let hundred = seed(&world, &team, m, ("100.00", "EUR", "MEALS", "DRAFT", "2026-01-03"), 2).await;
        seed(&world, &team, m, ("1.00", "USD", "MEALS", "DRAFT", "2026-01-04"), 3).await;

        assert_eq!(
            ids(&list(&world, &team, &team.approver, "sort=amount,asc&currency=EUR").await),
            strings(&[nine, ten, hundred])
        );
    }

    /// Java: `ExpensesListApiTest.sortInjectionIsRejected`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn sort_injection_is_rejected() {
        let world = World::start().await;
        let team = world.team().await;

        for sort in [
            "password",
            "claimantId",
            "id.value,asc",
            "incurredOn,sideways",
            "money.amount",
            "a,b,c",
        ] {
            assert_eq!(
                list(&world, &team, &team.approver, &format!("sort={sort}"))
                    .await
                    .status,
                400,
                "{sort}"
            );
        }
    }

    // --- filters ---

    /// Java: `ExpensesListApiTest.filtersCombine`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn filters_combine() {
        let world = World::start().await;
        let team = world.team().await;
        let (alice, bob) = (&team.member, &team.colleague);
        let target = seed(
            &world,
            &team,
            bob,
            ("20.00", "USD", "TRAVEL", "SUBMITTED", "2026-01-10"),
            0,
        )
        .await;
        seed(&world, &team, bob, ("20.00", "USD", "TRAVEL", "DRAFT", "2026-01-10"), 1).await;
        seed(
            &world,
            &team,
            bob,
            ("20.00", "USD", "MEALS", "SUBMITTED", "2026-01-10"),
            2,
        )
        .await;
        seed(
            &world,
            &team,
            bob,
            ("20.00", "EUR", "TRAVEL", "SUBMITTED", "2026-01-10"),
            3,
        )
        .await;
        seed(
            &world,
            &team,
            alice,
            ("20.00", "USD", "TRAVEL", "SUBMITTED", "2026-01-10"),
            4,
        )
        .await;
        seed(
            &world,
            &team,
            bob,
            ("20.00", "USD", "TRAVEL", "SUBMITTED", "2025-12-31"),
            5,
        )
        .await;

        let query = format!(
            "status=SUBMITTED&category=TRAVEL&currency=USD&claimantId={}&incurredFrom=2026-01-01&incurredTo=2026-01-31",
            bob.id()
        );

        assert_eq!(
            ids(&list(&world, &team, &team.approver, &query).await),
            strings(&[target])
        );
    }

    /// Java: `ExpensesListApiTest.dateRangeIsInclusive`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn date_range_is_inclusive() {
        let world = World::start().await;
        let team = world.team().await;
        let a = draft(&world, &team, &team.member, "2026-01-01", 0).await;
        let b = draft(&world, &team, &team.member, "2026-01-31", 1).await;
        draft(&world, &team, &team.member, "2026-02-01", 2).await;

        let query = "incurredFrom=2026-01-01&incurredTo=2026-01-31&sort=incurredOn,asc";

        assert_eq!(ids(&list(&world, &team, &team.approver, query).await), strings(&[a, b]));
    }

    /// Java: `ExpensesListApiTest.badFilterValuesAreRejected`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn bad_filter_values_are_rejected() {
        let world = World::start().await;
        let team = world.team().await;

        for query in [
            "status=BOGUS",
            "currency=ZZZ",
            "currency=eur",
            "incurredFrom=2026-02-01&incurredTo=2026-01-01",
            "incurredFrom=yesterday",
        ] {
            assert_eq!(list(&world, &team, &team.approver, query).await.status, 400, "{query}");
        }
    }

    // --- visibility (API1) ---

    /// Java: `ExpensesListApiTest.memberSeesOnlyOwnEvenWhenFilteringForSomeoneElse`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn member_sees_only_own_even_when_filtering_for_someone_else() {
        let world = World::start().await;
        let team = world.team().await;
        let mine = draft(&world, &team, &team.member, "2026-01-01", 0).await;
        draft(&world, &team, &team.colleague, "2026-01-01", 1).await;

        let all = list(&world, &team, &team.member, "").await;
        let filtered = list(
            &world,
            &team,
            &team.member,
            &format!("claimantId={}", team.colleague.id()),
        )
        .await;

        assert_eq!(ids(&all), strings(&[mine]));
        assert_eq!(ids(&filtered), strings(&[mine]));
        assert_eq!(all.json()["page"]["totalElements"], 1);
    }

    /// Java: `ExpensesListApiTest.approverAndOwnerSeeEveryone`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn approver_and_owner_see_everyone() {
        let world = World::start().await;
        let team = world.team().await;
        draft(&world, &team, &team.member, "2026-01-01", 0).await;
        draft(&world, &team, &team.colleague, "2026-01-01", 1).await;

        assert_eq!(ids(&list(&world, &team, &team.approver, "").await).len(), 2);
        assert_eq!(ids(&list(&world, &team, &team.owner, "").await).len(), 2);
    }

    /// Java: `ExpensesListApiTest.nonTeammateGets404`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn non_teammate_gets_404() {
        let world = World::start().await;
        let team = world.team().await;
        draft(&world, &team, &team.member, "2026-01-01", 0).await;
        let outsider = world.user().await;

        assert_eq!(list(&world, &team, &outsider, "").await.status, 404);
    }

    /// Java: `ExpensesListApiTest.anotherTeamsExpensesAreNotListed`.
    #[tokio::test]
    #[ignore = "M4"]
    async fn another_teams_expenses_are_not_listed() {
        let world = World::start().await;
        let team = world.team().await;
        let mine = draft(&world, &team, &team.member, "2026-01-01", 0).await;
        let other_team = world.fixtures.team().await;
        world
            .fixtures
            .insert_expense(&Expense {
                incurred_on: "2026-01-01".parse().unwrap(),
                created_at: t0(),
                ..Expense::new(other_team, team.member.id(), "DRAFT")
            })
            .await;

        assert_eq!(ids(&list(&world, &team, &team.approver, "").await), strings(&[mine]));
    }
}
