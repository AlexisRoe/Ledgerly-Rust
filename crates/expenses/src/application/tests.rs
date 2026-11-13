//! The use cases against real Postgres (plan M2.4, M3.4).
//!
//! In-crate, because the use cases are `pub(crate)` (ADR 0010). Java: `UpdateExpenseTest` and the use-case side
//! of `ExpensesApiTest`.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::money::Money;
use ledgerly_shared::role::Role;
use ledgerly_shared::version::{Version, Versioned};
use sqlx::PgPool;

use super::doubles::{FixedReceipts, Memberships};
use super::*;
use crate::ReceiptSummary;
use crate::domain::{Action, Category, Expense, ExpenseStatus, InvalidInput, NewExpense};

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

/// A team with a member, a second member, an approver and an owner, and the use cases wired to it.
struct World {
    pool: PgPool,
    team: TeamId,
    member: UserId,
    colleague: UserId,
    approver: UserId,
    owner: UserId,
    memberships: Arc<dyn MembershipLookup>,
}

impl World {
    fn new(database: &TestDatabase) -> Self {
        let team = TeamId::new_v7();
        let (member, colleague, approver, owner) =
            (UserId::new_v7(), UserId::new_v7(), UserId::new_v7(), UserId::new_v7());
        let memberships = Memberships::default()
            .with(team, member, Role::Member)
            .with(team, colleague, Role::Member)
            .with(team, approver, Role::Approver)
            .with(team, owner, Role::Owner);
        Self {
            pool: database.pool().clone(),
            team,
            member,
            colleague,
            approver,
            owner,
            memberships: Arc::new(memberships),
        }
    }

    fn create(&self) -> CreateExpense {
        CreateExpense::new(self.pool.clone(), clock(), self.memberships.clone())
    }

    fn get(&self) -> GetExpense {
        GetExpense::new(self.pool.clone(), self.memberships.clone())
    }

    fn update(&self) -> UpdateExpense {
        UpdateExpense::new(self.pool.clone(), clock(), self.memberships.clone())
    }

    fn delete(&self) -> DeleteExpense {
        DeleteExpense::new(self.pool.clone(), self.memberships.clone())
    }

    fn transition(&self, accepted_receipts: u32) -> TransitionExpense {
        TransitionExpense::new(
            self.pool.clone(),
            clock(),
            self.memberships.clone(),
            Arc::new(FixedReceipts(ReceiptSummary::new(accepted_receipts, 0))),
        )
    }

    async fn draft(&self) -> Versioned<Expense> {
        self.create()
            .execute(CreateExpenseCommand {
                team: self.team,
                claimant: self.member,
                expense: lunch(),
            })
            .await
            .unwrap()
    }

    fn edit(&self, id: ExpenseId, caller: UserId, expected: Version) -> UpdateExpenseCommand {
        UpdateExpenseCommand {
            team: self.team,
            id,
            caller,
            expected,
            money: None,
            category: None,
            incurred_on: None,
            merchant: None,
            note: None,
        }
    }

    async fn rows(&self) -> i64 {
        sqlx::query_scalar("select count(*) from expenses")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn updated_at(&self, id: ExpenseId) -> DateTime<Utc> {
        sqlx::query_scalar("select updated_at from expenses where id = $1")
            .bind(id.as_uuid())
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

/// The field rule the use case refused with, or a panic.
fn invalid(outcome: Result<impl std::fmt::Debug, UseCaseError>) -> InvalidInput {
    match outcome {
        Err(UseCaseError::Refused(ExpenseError::InvalidInput(invalid))) => invalid,
        other => panic!("expected a field rule to refuse, got {other:?}"),
    }
}

// --- create and get ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn create_stores_a_draft_for_the_caller_at_version_0_and_get_reads_it_back() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);

    let created = world.draft().await;
    let read = world
        .get()
        .execute(world.team, created.value.id(), world.member)
        .await
        .unwrap();

    assert_eq!(created.version, Version::INITIAL);
    assert_eq!(read, created);
    let expense = &read.value;
    assert_eq!(expense.team_id(), world.team);
    assert_eq!(expense.claimant_id(), world.member);
    assert_eq!(expense.status(), ExpenseStatus::Draft);
    assert_eq!(expense.money(), Money::parse("12.50", "EUR").unwrap());
    assert_eq!(
        expense.merchant().as_str(),
        "Café Müller",
        "the merchant is stored stripped"
    );
    let (created_at, updated_at): (DateTime<Utc>, DateTime<Utc>) =
        sqlx::query_as("select created_at, updated_at from expenses where id = $1")
            .bind(expense.id().as_uuid())
            .fetch_one(&world.pool)
            .await
            .unwrap();
    assert_eq!(
        (created_at, updated_at),
        (now(), now()),
        "the time comes from the clock"
    );
}

#[tokio::test]
async fn a_non_teammate_cannot_create_and_nothing_is_written() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);

    let outcome = world
        .create()
        .execute(CreateExpenseCommand {
            team: world.team,
            claimant: UserId::new_v7(),
            expense: lunch(),
        })
        .await;

    assert!(matches!(outcome, Err(UseCaseError::NotFound)), "{outcome:?}");
    assert_eq!(world.rows().await, 0);
}

#[tokio::test]
async fn field_rules_refuse_in_javas_order_and_write_nothing() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let use_case = world.create();
    let create = |expense| {
        use_case.execute(CreateExpenseCommand {
            team: world.team,
            claimant: world.member,
            expense,
        })
    };

    // U+00A0 is not whitespace to Java's `String.strip`, so that merchant is not blank. A tab is.
    assert!(
        create(NewExpense {
            merchant: " \u{a0} ".to_owned(),
            ..lunch()
        })
        .await
        .is_ok()
    );
    let tab = invalid(
        create(NewExpense {
            merchant: "\t".to_owned(),
            ..lunch()
        })
        .await,
    );
    assert_eq!((tab.field, tab.message.as_str()), ("merchant", "Merchant is required"));
    let future = invalid(
        create(NewExpense {
            incurred_on: NaiveDate::from_ymd_opt(2026, 11, 6).unwrap(),
            ..lunch()
        })
        .await,
    );
    assert_eq!(
        (future.field, future.message.as_str()),
        ("incurredOn", "incurredOn must not be in the future")
    );
    let everything = invalid(
        create(NewExpense {
            merchant: String::new(),
            note: Some("x".repeat(2001)),
            incurred_on: NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(),
            ..lunch()
        })
        .await,
    );
    assert_eq!(everything.field, "merchant");
    assert_eq!(world.rows().await, 1);
}

#[tokio::test]
async fn get_hides_what_the_caller_may_not_see() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let created = world.draft().await;
    let id = created.value.id();
    let get = world.get();

    assert!(
        get.execute(world.team, id, world.approver).await.is_ok(),
        "an approver sees the team's"
    );
    assert!(get.execute(world.team, id, world.owner).await.is_ok());
    for (team, caller) in [
        (world.team, world.colleague),
        (world.team, UserId::new_v7()),
        (TeamId::new_v7(), world.member),
    ] {
        let outcome = get.execute(team, id, caller).await;
        assert!(matches!(outcome, Err(UseCaseError::NotFound)), "{outcome:?}");
    }
    let missing = get.execute(world.team, ExpenseId::new_v7(), world.member).await;
    assert!(matches!(missing, Err(UseCaseError::NotFound)));
}

// --- update -----------------------------------------------------------------------------------------------------

/// Java: `UpdateExpenseTest.onlyTheFieldsThatWereSentChange`.
#[tokio::test]
async fn only_the_fields_sent_change_and_the_version_moves() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let created = world.draft().await;

    let updated = world
        .update()
        .execute(UpdateExpenseCommand {
            merchant: Some("Bakery".to_owned()),
            note: Some("rolls".to_owned()),
            ..world.edit(created.value.id(), world.member, created.version)
        })
        .await
        .unwrap();

    assert_eq!(updated.version, Version::from_stored(1));
    assert_eq!(updated.value.merchant().as_str(), "Bakery");
    assert_eq!(updated.value.note().map(|n| n.as_str()), Some("rolls"));
    assert_eq!(updated.value.money(), created.value.money());
    assert_eq!(updated.value.category(), created.value.category());
    assert_eq!(
        world
            .get()
            .execute(world.team, created.value.id(), world.member)
            .await
            .unwrap(),
        updated
    );
}

/// Java: `UpdateExpenseTest.blankNoteClearsIt`.
#[tokio::test]
async fn a_blank_note_clears_the_note() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let created = world.draft().await;

    let updated = world
        .update()
        .execute(UpdateExpenseCommand {
            note: Some("  ".to_owned()),
            ..world.edit(created.value.id(), world.member, created.version)
        })
        .await
        .unwrap();

    assert_eq!(updated.value.note(), None);
}

/// Hibernate's dirty checking writes nothing when nothing changed (decided after M2): the ETag and `updated_at`
/// stay. Java: `UpdateExpenseTest.sameNoteIsNoChange`.
#[tokio::test]
async fn an_edit_that_changes_nothing_writes_nothing() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let created = world.draft().await;
    let id = created.value.id();
    let later = UpdateExpense::new(
        world.pool.clone(),
        Arc::new(FixedClock::at(now() + chrono::TimeDelta::hours(1))),
        world.memberships.clone(),
    );

    for command in [
        world.edit(id, world.member, created.version),
        UpdateExpenseCommand {
            note: Some("Team lunch".to_owned()),
            merchant: Some(" Café Müller".to_owned()),
            money: Some(Money::parse("12.500", "EUR").unwrap()),
            ..world.edit(id, world.member, created.version)
        },
    ] {
        let outcome = later.execute(command).await.unwrap();
        assert_eq!(outcome.version, Version::INITIAL);
    }
    assert_eq!(world.updated_at(id).await, now());
}

/// Java: `UpdateExpenseTest` (stale version, non-claimant owner, outsider, future date change nothing).
#[tokio::test]
async fn refused_edits_change_nothing_in_javas_order() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let created = world.draft().await;
    let id = created.value.id();
    let update = world.update();
    let renamed = |caller, expected| UpdateExpenseCommand {
        merchant: Some("Other".to_owned()),
        ..world.edit(id, caller, expected)
    };

    let outsider = update.execute(renamed(UserId::new_v7(), created.version)).await;
    assert!(matches!(outsider, Err(UseCaseError::NotFound)), "{outsider:?}");
    // The version is checked before the role: a stale owner gets 412, not 403.
    let stale = update.execute(renamed(world.owner, Version::from_stored(7))).await;
    assert!(matches!(stale, Err(UseCaseError::Stale)), "{stale:?}");
    let owner = update.execute(renamed(world.owner, created.version)).await;
    assert!(
        matches!(owner, Err(UseCaseError::Refused(ExpenseError::Forbidden(_)))),
        "{owner:?}"
    );
    let future = update
        .execute(UpdateExpenseCommand {
            incurred_on: NaiveDate::from_ymd_opt(2026, 11, 6),
            ..world.edit(id, world.member, created.version)
        })
        .await;
    assert_eq!(invalid(future).field, "incurredOn");

    assert_eq!(
        world.get().execute(world.team, id, world.member).await.unwrap(),
        created
    );
}

#[tokio::test]
async fn only_a_draft_can_be_edited_or_deleted() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let created = world.draft().await;
    let id = created.value.id();
    let submitted = world
        .transition(1)
        .execute(Action::Submit, world.team, id, world.member, None)
        .await
        .unwrap();

    let edit = world
        .update()
        .execute(world.edit(id, world.member, submitted.version))
        .await;
    let delete = world
        .delete()
        .execute(world.team, id, world.member, submitted.version)
        .await;

    for outcome in [edit.map(|_| ()), delete] {
        assert!(
            matches!(outcome, Err(UseCaseError::Refused(ExpenseError::NotDraft { .. }))),
            "{outcome:?}"
        );
    }
}

// --- delete -----------------------------------------------------------------------------------------------------

#[tokio::test]
async fn delete_needs_the_claimant_and_the_current_version() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let created = world.draft().await;
    let id = created.value.id();
    let delete = world.delete();

    let stale = delete
        .execute(world.team, id, world.member, Version::from_stored(1))
        .await;
    assert!(matches!(stale, Err(UseCaseError::Stale)), "{stale:?}");
    let approver = delete.execute(world.team, id, world.approver, created.version).await;
    assert!(
        matches!(approver, Err(UseCaseError::Refused(ExpenseError::Forbidden(_)))),
        "{approver:?}"
    );
    let colleague = delete.execute(world.team, id, world.colleague, created.version).await;
    assert!(matches!(colleague, Err(UseCaseError::NotFound)), "{colleague:?}");

    delete
        .execute(world.team, id, world.member, created.version)
        .await
        .unwrap();
    assert_eq!(world.rows().await, 0);
}

// --- transitions ------------------------------------------------------------------------------------------------

/// Java: `ExpensesApiTest.fullWorkflowApproveAndReimburse` and `rejectReopenSubmitWithdraw`.
#[tokio::test]
async fn the_workflow_runs_through_every_transition_and_each_bumps_the_version() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let id = world.draft().await.value.id();
    let transition = world.transition(1);
    let steps = [
        (Action::Submit, world.member, None, ExpenseStatus::Submitted),
        (Action::Reject, world.approver, Some("blurry"), ExpenseStatus::Rejected),
        (Action::Reopen, world.member, None, ExpenseStatus::Draft),
        (Action::Submit, world.member, None, ExpenseStatus::Submitted),
        (Action::Withdraw, world.member, None, ExpenseStatus::Draft),
        (Action::Submit, world.member, None, ExpenseStatus::Submitted),
        (Action::Approve, world.approver, None, ExpenseStatus::Approved),
        (Action::Reimburse, world.owner, None, ExpenseStatus::Reimbursed),
    ];

    for (number, (action, caller, reason, status)) in (1_i64..).zip(steps) {
        let after = transition
            .execute(action, world.team, id, caller, reason)
            .await
            .unwrap();
        assert_eq!(after.value.status(), status, "{action}");
        assert_eq!(after.version, Version::from_stored(number), "{action}");
    }
    let done = world.get().execute(world.team, id, world.owner).await.unwrap();
    let reasons: Vec<&str> = done.value.rejections().iter().map(|r| r.reason().as_str()).collect();
    assert_eq!(reasons, ["blurry"]);
}

#[tokio::test]
async fn transitions_are_refused_as_java_refuses_them() {
    let database = TestDatabase::new().await.unwrap();
    let world = World::new(&database);
    let id = world.draft().await.value.id();
    let refused = |outcome: Result<Versioned<Expense>, UseCaseError>| match outcome {
        Err(UseCaseError::Refused(error)) => error,
        other => panic!("expected a refusal, got {other:?}"),
    };

    let no_receipt = world
        .transition(0)
        .execute(Action::Submit, world.team, id, world.member, None)
        .await;
    assert!(matches!(refused(no_receipt), ExpenseError::RuleViolated(_)));
    let approve_draft = world
        .transition(1)
        .execute(Action::Approve, world.team, id, world.approver, None)
        .await;
    assert!(matches!(refused(approve_draft), ExpenseError::InvalidTransition { .. }));
    let member_approves = world
        .transition(1)
        .execute(Action::Approve, world.team, id, world.member, None)
        .await;
    assert!(matches!(refused(member_approves), ExpenseError::Forbidden(_)));
    let colleague = world
        .transition(1)
        .execute(Action::Submit, world.team, id, world.colleague, None)
        .await;
    assert!(matches!(colleague, Err(UseCaseError::NotFound)));

    // An approver's own expense: self-dealing once it is submitted.
    let own = world
        .create()
        .execute(CreateExpenseCommand {
            team: world.team,
            claimant: world.approver,
            expense: lunch(),
        })
        .await
        .unwrap()
        .value
        .id();
    world
        .transition(1)
        .execute(Action::Submit, world.team, own, world.approver, None)
        .await
        .unwrap();
    let self_approval = world
        .transition(1)
        .execute(Action::Approve, world.team, own, world.approver, None)
        .await;
    assert!(matches!(refused(self_approval), ExpenseError::SelfDealing { .. }));
}
