//! `GET /teams/{teamId}/expenses` in-process against real Postgres (plan M4.1).
//!
//! Java: `ExpensesListApiTest`, whose black-box twin is `crates/parity-tests/tests/expenses_list.rs`. That one
//! needs signed-in teammates (M5, M6); this one puts the caller into the request and takes memberships from a
//! test double, as the other in-process tests do. Expenses are seeded straight into the table, so that
//! `created_at`, status and claimant are what the test says.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::Router;
use axum::body::Body as HttpBody;
use axum::http::{Request, StatusCode};
use chrono::{DateTime, NaiveDate, TimeDelta, TimeZone, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::Role;
use ledgerly_shared::web::{CurrentUser, render};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

use super::ExpensesApi;
use crate::application::MembershipLookup;
use crate::application::doubles::{FixedReceipts, Memberships};
use crate::{ReceiptChecks, ReceiptSummary};

struct World {
    database: TestDatabase,
    router: Router,
    team: TeamId,
    member: UserId,
    colleague: UserId,
    approver: UserId,
    owner: UserId,
}

struct Reply {
    status: StatusCode,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or_default()
    }

    fn ids(&self) -> Vec<String> {
        self.body["content"]
            .as_array()
            .unwrap_or_else(|| panic!("no content in {}", self.body))
            .iter()
            .map(|item| item["id"].as_str().unwrap().to_owned())
            .collect()
    }

    /// `field: message` of every entry of `errors`.
    fn errors(&self) -> Vec<String> {
        self.body["errors"]
            .as_array()
            .map(|errors| {
                errors
                    .iter()
                    .map(|error| {
                        format!(
                            "{}: {}",
                            error["field"].as_str().unwrap(),
                            error["message"].as_str().unwrap()
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Java: `seed()`'s columns.
struct Seed<'a> {
    amount: &'a str,
    currency: &'a str,
    category: &'a str,
    status: &'a str,
    incurred_on: &'a str,
}

const LUNCH: Seed<'static> = Seed {
    amount: "1.00",
    currency: "EUR",
    category: "MEALS",
    status: "DRAFT",
    incurred_on: "2026-01-15",
};

/// Java: `t0`.
fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap()
}

impl World {
    async fn new() -> Self {
        Self::with_max_page_size(100).await
    }

    async fn with_max_page_size(max_page_size: u32) -> Self {
        let database = TestDatabase::new().await.unwrap();
        let team = TeamId::new_v7();
        let (member, colleague, approver, owner) =
            (UserId::new_v7(), UserId::new_v7(), UserId::new_v7(), UserId::new_v7());
        let memberships: Arc<dyn MembershipLookup> = Arc::new(
            Memberships::default()
                .with(team, member, Role::Member)
                .with(team, colleague, Role::Member)
                .with(team, approver, Role::Approver)
                .with(team, owner, Role::Owner),
        );
        let receipts: Arc<dyn ReceiptChecks> = Arc::new(FixedReceipts(ReceiptSummary::new(1, 0)));
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, 0).unwrap()));
        let routes = ExpensesApi::new(database.pool(), &clock, &memberships, &receipts, max_page_size).routes();
        let router = Router::new()
            .fallback_service(routes.into_service())
            .layer(axum::middleware::from_fn(render));
        Self {
            database,
            router,
            team,
            member,
            colleague,
            approver,
            owner,
        }
    }

    fn pool(&self) -> &PgPool {
        self.database.pool()
    }

    /// Java: `seed()`: an expense of `claimant` in `team`, created `offset` seconds after `t0`.
    async fn seed_in(&self, team: TeamId, claimant: UserId, seed: &Seed<'_>, offset: i64) -> String {
        let id = Uuid::now_v7();
        let created_at = t0() + TimeDelta::seconds(offset);
        sqlx::query(
            "insert into expenses (id, team_id, claimant_id, amount, currency, category, incurred_on, merchant, \
                                   status, version, created_at, updated_at) \
             values ($1, $2, $3, $4::numeric, $5, $6, $7, 'Café', $8, 0, $9, $9)",
        )
        .bind(id)
        .bind(team.as_uuid())
        .bind(claimant.as_uuid())
        .bind(seed.amount)
        .bind(seed.currency)
        .bind(seed.category)
        .bind(seed.incurred_on.parse::<NaiveDate>().unwrap())
        .bind(seed.status)
        .bind(created_at)
        .execute(self.pool())
        .await
        .unwrap();
        id.to_string()
    }

    async fn seed(&self, claimant: UserId, seed: &Seed<'_>, offset: i64) -> String {
        self.seed_in(self.team, claimant, seed, offset).await
    }

    /// A draft of 1.00 EUR, MEALS, incurred on `date`.
    async fn draft(&self, claimant: UserId, date: &str, offset: i64) -> String {
        self.seed(
            claimant,
            &Seed {
                incurred_on: date,
                ..LUNCH
            },
            offset,
        )
        .await
    }

    async fn list(&self, caller: UserId, query: &str) -> Reply {
        self.list_as(Some(caller), query).await
    }

    async fn list_as(&self, caller: Option<UserId>, query: &str) -> Reply {
        let query = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let mut request = Request::get(format!("/api/v1/teams/{}/expenses{query}", self.team))
            .body(HttpBody::empty())
            .unwrap();
        if let Some(caller) = caller {
            request.extensions_mut().insert(CurrentUser::new(caller));
        }
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply { status, body }
    }
}

// --- paging ---

/// Java: `pagingThrough250ExpensesVisitsEachOnce`.
#[tokio::test]
async fn paging_through_250_expenses_visits_each_once() {
    let world = World::new().await;
    let mut expected = BTreeSet::new();
    for offset in 0..250 {
        expected.insert(world.draft(world.member, "2026-01-15", offset).await);
    }

    let mut seen = Vec::new();
    for page in 0..3 {
        let reply = world.list(world.approver, &format!("size=100&page={page}")).await;
        assert_eq!(reply.status, StatusCode::OK);
        let meta = &reply.body["page"];
        assert_eq!(meta["totalElements"], 250);
        assert_eq!(meta["totalPages"], 3);
        assert_eq!(meta["size"], 100);
        assert_eq!(meta["number"], page);
        seen.extend(reply.ids());
    }

    assert_eq!(seen.len(), 250);
    assert_eq!(seen.into_iter().collect::<BTreeSet<_>>(), expected);
}

/// Java: `pageBeyondTheEndIsEmptyNotAnError`. The totals still count what is there.
#[tokio::test]
async fn a_page_beyond_the_end_is_empty_not_an_error() {
    let world = World::new().await;
    world.draft(world.member, "2026-01-15", 0).await;

    let reply = world.list(world.approver, "page=5").await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.ids().is_empty());
    assert_eq!(reply.body["page"]["totalElements"], 1);
    assert_eq!(reply.body["page"]["totalPages"], 1);
    assert_eq!(reply.body["page"]["number"], 5);
    assert_eq!(reply.body["page"]["size"], 20);
}

/// Java: `sizeAbove100IsRejectedNotClamped_API4`, with the message of the generated `@Max(100)`.
#[tokio::test]
async fn a_size_above_100_is_rejected_not_clamped() {
    let world = World::new().await;

    let reply = world.list(world.approver, "size=101").await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.code(), "VALIDATION_FAILED");
    assert_eq!(reply.errors(), ["size: must be less than or equal to 100"]);
}

/// Java: `zeroNegativeAndNonNumericPagingParametersAreRejected`.
#[tokio::test]
async fn zero_negative_and_non_numeric_paging_parameters_are_rejected() {
    let world = World::new().await;

    let zero = world.list(world.approver, "size=0").await;
    let negative = world.list(world.approver, "page=-1").await;
    let text = world.list(world.approver, "size=abc").await;

    assert_eq!(zero.status, StatusCode::BAD_REQUEST);
    assert_eq!(zero.errors(), ["size: must be greater than or equal to 1"]);
    assert_eq!(negative.status, StatusCode::BAD_REQUEST);
    assert_eq!(negative.errors(), ["page: must be greater than or equal to 0"]);
    assert_eq!(text.status, StatusCode::BAD_REQUEST);
    assert_eq!(text.code(), "REQUEST_REJECTED");
    assert_eq!(text.body["detail"], "Failed to convert 'size' with value: 'abc'");
}

/// `ledgerly.limits.max-page-size` is checked as well as the spec's 100, by the use case.
#[tokio::test]
async fn a_lower_configured_page_size_limit_is_enforced_too() {
    let world = World::with_max_page_size(10).await;

    let reply = world.list(world.approver, "size=11").await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.errors(), ["size: size must be between 1 and 10"]);
}

// --- sorting ---

/// Java: `defaultSortIsIncurredOnDescending`.
#[tokio::test]
async fn the_default_sort_is_incurred_on_descending() {
    let world = World::new().await;
    let old = world.draft(world.member, "2026-01-01", 0).await;
    let recent = world.draft(world.member, "2026-01-20", 1).await;

    assert_eq!(world.list(world.approver, "").await.ids(), [recent, old]);
}

/// Java: `sortAscendingAndByCreatedAt`.
#[tokio::test]
async fn sort_ascending_and_by_created_at() {
    let world = World::new().await;
    let first = world.draft(world.member, "2026-01-20", 0).await;
    let second = world.draft(world.member, "2026-01-01", 1).await;

    assert_eq!(
        world.list(world.approver, "sort=incurredOn,asc").await.ids(),
        [second.clone(), first.clone()]
    );
    assert_eq!(
        world.list(world.approver, "sort=createdAt,desc").await.ids(),
        [second, first]
    );
}

/// Equal sort values are ordered by id in the same direction, so pages never overlap.
#[tokio::test]
async fn equal_sort_values_are_ordered_by_id() {
    let world = World::new().await;
    let mut ids = Vec::new();
    for offset in 0..5 {
        ids.push(world.draft(world.member, "2026-01-15", offset).await);
    }
    ids.sort();

    assert_eq!(world.list(world.approver, "sort=incurredOn,asc").await.ids(), ids);
    ids.reverse();
    assert_eq!(world.list(world.approver, "").await.ids(), ids);
}

/// A `String` parameter given twice is joined with a comma, as Spring does.
#[tokio::test]
async fn a_sort_given_in_two_parameters_is_joined() {
    let world = World::new().await;
    let old = world.draft(world.member, "2026-01-01", 0).await;
    let recent = world.draft(world.member, "2026-01-20", 1).await;

    assert_eq!(
        world.list(world.approver, "sort=incurredOn&sort=asc").await.ids(),
        [old, recent]
    );
}

/// Java: `sortByAmountNeedsACurrencyFilter`.
#[tokio::test]
async fn sort_by_amount_needs_a_currency_filter() {
    let world = World::new().await;
    world
        .seed(
            world.member,
            &Seed {
                amount: "5.00",
                ..LUNCH
            },
            0,
        )
        .await;

    let reply = world.list(world.approver, "sort=amount,asc").await;

    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.errors(), ["sort: Sorting by amount needs a currency filter"]);
}

/// Java: `sortByAmountWithCurrencyOrdersNumerically`.
#[tokio::test]
async fn sort_by_amount_with_currency_orders_numerically() {
    let world = World::new().await;
    let m = world.member;
    let ten = world
        .seed(
            m,
            &Seed {
                amount: "10.00",
                incurred_on: "2026-01-01",
                ..LUNCH
            },
            0,
        )
        .await;
    let nine = world
        .seed(
            m,
            &Seed {
                amount: "9.00",
                incurred_on: "2026-01-02",
                ..LUNCH
            },
            1,
        )
        .await;
    let hundred = world
        .seed(
            m,
            &Seed {
                amount: "100.00",
                incurred_on: "2026-01-03",
                ..LUNCH
            },
            2,
        )
        .await;
    world
        .seed(
            m,
            &Seed {
                currency: "USD",
                incurred_on: "2026-01-04",
                ..LUNCH
            },
            3,
        )
        .await;

    assert_eq!(
        world.list(world.approver, "sort=amount,asc&currency=EUR").await.ids(),
        [nine, ten, hundred]
    );
}

/// Java: `sortInjectionIsRejected`. The spec's pattern refuses all of them, with the generated `@Pattern`'s
/// message.
#[tokio::test]
async fn sort_injection_is_rejected() {
    let world = World::new().await;

    for sort in [
        "password",
        "claimantId",
        "id.value,asc",
        "incurredOn,sideways",
        "money.amount",
        "a,b,c",
        "incurredOn;drop+table+expenses",
        "incurredOn,asc,",
    ] {
        let reply = world.list(world.approver, &format!("sort={sort}")).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{sort}");
        assert_eq!(
            reply.errors(),
            [r#"sort: must match "^(incurredOn|createdAt|amount)(,(asc|desc))?$""#],
            "{sort}"
        );
    }
}

// --- filters ---

/// Java: `filtersCombine`.
#[tokio::test]
async fn filters_combine() {
    let world = World::new().await;
    let (alice, bob) = (world.member, world.colleague);
    let travel = Seed {
        amount: "20.00",
        currency: "USD",
        category: "TRAVEL",
        status: "SUBMITTED",
        incurred_on: "2026-01-10",
    };
    let target = world.seed(bob, &travel, 0).await;
    world
        .seed(
            bob,
            &Seed {
                status: "DRAFT",
                ..travel
            },
            1,
        )
        .await;
    world
        .seed(
            bob,
            &Seed {
                category: "MEALS",
                ..travel
            },
            2,
        )
        .await;
    world
        .seed(
            bob,
            &Seed {
                currency: "EUR",
                ..travel
            },
            3,
        )
        .await;
    world.seed(alice, &travel, 4).await;
    world
        .seed(
            bob,
            &Seed {
                incurred_on: "2025-12-31",
                ..travel
            },
            5,
        )
        .await;

    let query = format!(
        "status=SUBMITTED&category=TRAVEL&currency=USD&claimantId={bob}&incurredFrom=2026-01-01&incurredTo=2026-01-31"
    );

    assert_eq!(world.list(world.approver, &query).await.ids(), [target]);
}

/// Java: `dateRangeIsInclusive`.
#[tokio::test]
async fn the_date_range_is_inclusive() {
    let world = World::new().await;
    let a = world.draft(world.member, "2026-01-01", 0).await;
    let b = world.draft(world.member, "2026-01-31", 1).await;
    world.draft(world.member, "2026-02-01", 2).await;

    let query = "incurredFrom=2026-01-01&incurredTo=2026-01-31&sort=incurredOn,asc";

    assert_eq!(world.list(world.approver, query).await.ids(), [a, b]);
}

/// Spring's `@DateTimeFormat(iso = DATE)` reads an offset after the date and ignores it; a blank date is no
/// filter.
#[tokio::test]
async fn a_date_may_carry_an_offset_and_a_blank_one_is_no_filter() {
    let world = World::new().await;
    let a = world.draft(world.member, "2026-01-31", 0).await;
    let b = world.draft(world.member, "2026-02-01", 1).await;

    assert_eq!(
        world.list(world.approver, "incurredTo=2026-01-31Z").await.ids(),
        std::slice::from_ref(&a)
    );
    assert_eq!(world.list(world.approver, "incurredTo=+&status=").await.ids(), [b, a]);
}

/// Java: `badFilterValuesAreRejected`, with the problem each one gets.
#[tokio::test]
async fn bad_filter_values_are_rejected() {
    let world = World::new().await;

    let status = world.list(world.approver, "status=BOGUS").await;
    let unknown = world.list(world.approver, "currency=ZZZ").await;
    let lower = world.list(world.approver, "currency=eur").await;
    let range = world
        .list(world.approver, "incurredFrom=2026-02-01&incurredTo=2026-01-01")
        .await;
    let date = world.list(world.approver, "incurredFrom=yesterday").await;
    let claimant = world.list(world.approver, "claimantId=bob").await;

    for reply in [&status, &unknown, &lower, &range, &date, &claimant] {
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
    }
    assert_eq!(status.body["detail"], "Failed to convert 'status' with value: 'BOGUS'");
    assert_eq!(unknown.errors(), ["currency: Unknown currency code"]);
    assert_eq!(lower.errors(), [r#"currency: must match "^[A-Z]{3}$""#]);
    assert_eq!(
        range.errors(),
        ["incurredFrom: incurredFrom must not be after incurredTo"]
    );
    assert_eq!(
        date.body["detail"],
        "Failed to convert 'incurredFrom' with value: 'yesterday'"
    );
    assert_eq!(
        claimant.body["detail"],
        "Failed to convert 'claimantId' with value: 'bob'"
    );
    for reply in [&status, &date, &claimant] {
        assert_eq!(reply.code(), "REQUEST_REJECTED");
    }
}

/// Java's filter goes through `Currency.getInstance`, which knows XXX: no expense is in it, so nothing is
/// found, but it is not an error.
#[tokio::test]
async fn a_currency_without_minor_units_is_a_valid_filter_that_finds_nothing() {
    let world = World::new().await;
    world.draft(world.member, "2026-01-01", 0).await;

    let reply = world.list(world.approver, "currency=XXX").await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.ids().is_empty());
}

// --- visibility (API1) ---

/// Java: `memberSeesOnlyOwnEvenWhenFilteringForSomeoneElse`.
#[tokio::test]
async fn a_member_sees_only_their_own_even_when_filtering_for_someone_else() {
    let world = World::new().await;
    let mine = world.draft(world.member, "2026-01-01", 0).await;
    world.draft(world.colleague, "2026-01-01", 1).await;

    let all = world.list(world.member, "").await;
    let filtered = world
        .list(world.member, &format!("claimantId={}", world.colleague))
        .await;

    assert_eq!(all.ids(), std::slice::from_ref(&mine));
    assert_eq!(filtered.ids(), [mine]);
    assert_eq!(all.body["page"]["totalElements"], 1);
}

/// Java: `approverAndOwnerSeeEveryone`.
#[tokio::test]
async fn approver_and_owner_see_everyone() {
    let world = World::new().await;
    world.draft(world.member, "2026-01-01", 0).await;
    world.draft(world.colleague, "2026-01-01", 1).await;

    assert_eq!(world.list(world.approver, "").await.ids().len(), 2);
    assert_eq!(world.list(world.owner, "").await.ids().len(), 2);
}

/// Java: `nonTeammateGets404`.
#[tokio::test]
async fn a_non_teammate_gets_404() {
    let world = World::new().await;
    world.draft(world.member, "2026-01-01", 0).await;

    let reply = world.list(UserId::new_v7(), "").await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.code(), "NOT_FOUND");
}

/// Java: `anotherTeamsExpensesAreNotListed`.
#[tokio::test]
async fn another_teams_expenses_are_not_listed() {
    let world = World::new().await;
    let mine = world.draft(world.member, "2026-01-01", 0).await;
    world.seed_in(TeamId::new_v7(), world.member, &LUNCH, 0).await;

    assert_eq!(world.list(world.approver, "").await.ids(), [mine]);
}

// --- order of checks and shape ---

/// Spring converts and validates the parameters before the controller runs; the use case checks membership
/// before its own rules.
#[tokio::test]
async fn parameters_are_checked_before_membership_and_the_use_case_rules_after() {
    let world = World::new().await;
    let stranger = UserId::new_v7();

    assert_eq!(world.list_as(None, "size=abc").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(world.list(stranger, "size=abc").await.status, StatusCode::BAD_REQUEST);
    assert_eq!(world.list(stranger, "size=101").await.status, StatusCode::BAD_REQUEST);
    assert_eq!(world.list(stranger, "currency=ZZZ").await.status, StatusCode::NOT_FOUND);
    assert_eq!(world.list(stranger, "sort=amount").await.status, StatusCode::NOT_FOUND);
}

/// The first parameter that does not convert is the one reported; constraint violations are all reported.
#[tokio::test]
async fn the_first_unconvertible_parameter_wins_and_constraints_are_reported_together() {
    let world = World::new().await;

    let conversion = world.list(world.approver, "incurredTo=x&status=x&size=x").await;
    let constraints = world.list(world.approver, "currency=eur&size=0&page=-1&sort=x").await;

    assert_eq!(conversion.body["detail"], "Failed to convert 'size' with value: 'x'");
    assert_eq!(
        constraints.errors(),
        [
            "page: must be greater than or equal to 0",
            "size: must be greater than or equal to 1",
            r#"sort: must match "^(incurredOn|createdAt|amount)(,(asc|desc))?$""#,
            r#"currency: must match "^[A-Z]{3}$""#,
        ]
    );
}

/// `{content, page}` as Spring Data's `PagedModel`, each item the expense as `GET` shows it.
#[tokio::test]
async fn the_page_has_spring_datas_shape() {
    let world = World::new().await;
    let id = world.draft(world.member, "2026-01-01", 0).await;

    let reply = world.list(world.approver, "").await;

    let keys = |value: &Value| value.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(keys(&reply.body), ["content", "page"].map(String::from).into());
    assert_eq!(
        keys(&reply.body["page"]),
        ["number", "size", "totalElements", "totalPages"]
            .map(String::from)
            .into()
    );
    let item = &reply.body["content"][0];
    assert_eq!(item["id"], id.as_str());
    assert_eq!(item["teamId"], world.team.to_string());
    assert_eq!(item["money"]["amount"], "1.00");
    assert_eq!(item["status"], "DRAFT");
    assert_eq!(item["rejectionReasons"], Value::Array(Vec::new()));
    assert_eq!(
        keys(item),
        [
            "category",
            "claimantId",
            "id",
            "incurredOn",
            "merchant",
            "money",
            "note",
            "rejectionReasons",
            "status",
            "teamId"
        ]
        .map(String::from)
        .into()
    );
}

/// Rejections come along, oldest first, as on `GET`.
#[tokio::test]
async fn listed_expenses_carry_their_rejections_in_order() {
    let world = World::new().await;
    let id = world
        .seed(
            world.member,
            &Seed {
                status: "REJECTED",
                ..LUNCH
            },
            0,
        )
        .await;
    for (position, reason) in ["first", "second"].iter().enumerate() {
        sqlx::query(
            "insert into expense_rejections (expense_id, position, rejected_by, reason) values ($1::uuid, $2, $3, $4)",
        )
        .bind(&id)
        .bind(i32::try_from(position).unwrap())
        .bind(world.approver.as_uuid())
        .bind(*reason)
        .execute(world.pool())
        .await
        .unwrap();
    }

    let reply = world.list(world.approver, "").await;

    assert_eq!(
        reply.body["content"][0]["rejectionReasons"],
        serde_json::json!(["first", "second"])
    );
}
