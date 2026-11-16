//! Java: `EveryEventReachesTheAuditTrailTest`, `AuditTrailTest` and `AuditModuleTest`.

use chrono::{TimeZone, Utc};
use ledgerly_shared::clock::FixedClock;
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::id::{ExpenseId, ImportId, InvitationId, ReceiptId, TeamId, UserId};
use ledgerly_shared::role::Role;
use serde_json::Value;
use uuid::Uuid;

use super::*;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 16, 12, 0, 0).unwrap()
}

/// An event, and what Java records for it: its type, whether it has a team and an actor, and the details.
type Sample = (Box<dyn Event>, &'static str, bool, bool, Value);

/// One event of every kind there is.
fn samples(team: TeamId, user: UserId) -> Vec<Sample> {
    let at = now();
    let id = Uuid::now_v7;
    let family = Uuid::now_v7();
    let invitation = InvitationId::new_v7();
    let expense = ExpenseId::new_v7();
    let team_event = |kind| TeamEvent {
        event_id: id(),
        team_id: team,
        actor_id: user,
        occurred_at: at,
        kind,
    };
    let expense_event = |kind| ExpenseEvent {
        event_id: id(),
        team_id: team,
        expense_id: expense,
        actor_id: user,
        occurred_at: at,
        kind,
    };
    let receipt_event = |kind| ReceiptEvent {
        event_id: id(),
        team_id: team,
        expense_id: expense,
        receipt_id: ReceiptId::new_v7(),
        occurred_at: at,
        kind,
    };
    let none = || serde_json::json!({});
    let on_expense = || serde_json::json!({ "expenseId": expense });
    vec![
        (
            Box::new(IdentityEvent::UserRegistered {
                event_id: id(),
                actor_id: user,
                occurred_at: at,
            }),
            "USER_REGISTERED",
            false,
            true,
            none(),
        ),
        (
            Box::new(IdentityEvent::UserVerified {
                event_id: id(),
                actor_id: user,
                occurred_at: at,
            }),
            "USER_VERIFIED",
            false,
            true,
            none(),
        ),
        (
            Box::new(IdentityEvent::RefreshTokenReuseDetected {
                event_id: id(),
                actor_id: user,
                family_id: family,
                occurred_at: at,
            }),
            "REFRESH_TOKEN_REUSE_DETECTED",
            false,
            true,
            serde_json::json!({ "familyId": family }),
        ),
        (
            Box::new(team_event(TeamEventKind::TeamCreated)),
            "TEAM_CREATED",
            true,
            true,
            none(),
        ),
        (
            Box::new(team_event(TeamEventKind::MemberInvited {
                invitation_id: invitation,
                role: Role::Approver,
            })),
            "MEMBER_INVITED",
            true,
            true,
            serde_json::json!({ "role": "APPROVER" }),
        ),
        (
            Box::new(team_event(TeamEventKind::InvitationRevoked {
                invitation_id: invitation,
            })),
            "INVITATION_REVOKED",
            true,
            true,
            none(),
        ),
        (
            Box::new(team_event(TeamEventKind::InvitationAccepted {
                invitation_id: invitation,
            })),
            "INVITATION_ACCEPTED",
            true,
            true,
            none(),
        ),
        (
            Box::new(team_event(TeamEventKind::InvitationDeclined {
                invitation_id: invitation,
            })),
            "INVITATION_DECLINED",
            true,
            true,
            none(),
        ),
        (
            Box::new(team_event(TeamEventKind::MemberRoleChanged {
                member_id: user,
                old_role: Role::Member,
                new_role: Role::Owner,
            })),
            "MEMBER_ROLE_CHANGED",
            true,
            true,
            serde_json::json!({ "oldRole": "MEMBER", "newRole": "OWNER" }),
        ),
        (
            Box::new(team_event(TeamEventKind::MemberRemoved { member_id: user })),
            "MEMBER_REMOVED",
            true,
            true,
            none(),
        ),
        (
            Box::new(team_event(TeamEventKind::MemberLeft)),
            "MEMBER_LEFT",
            true,
            true,
            none(),
        ),
        (
            Box::new(expense_event(ExpenseEventKind::Created)),
            "EXPENSE_CREATED",
            true,
            true,
            none(),
        ),
        (
            Box::new(expense_event(ExpenseEventKind::Edited {
                changed_fields: vec!["merchant".to_owned(), "note".to_owned()],
            })),
            "EXPENSE_EDITED",
            true,
            true,
            serde_json::json!({ "changedFields": ["merchant", "note"] }),
        ),
        (
            Box::new(expense_event(ExpenseEventKind::Deleted)),
            "EXPENSE_DELETED",
            true,
            true,
            none(),
        ),
        (
            Box::new(expense_event(ExpenseEventKind::Submitted)),
            "EXPENSE_SUBMITTED",
            true,
            true,
            none(),
        ),
        (
            Box::new(expense_event(ExpenseEventKind::Withdrawn)),
            "EXPENSE_WITHDRAWN",
            true,
            true,
            none(),
        ),
        (
            Box::new(expense_event(ExpenseEventKind::Approved)),
            "EXPENSE_APPROVED",
            true,
            true,
            none(),
        ),
        (
            Box::new(expense_event(ExpenseEventKind::Rejected)),
            "EXPENSE_REJECTED",
            true,
            true,
            none(),
        ),
        (
            Box::new(expense_event(ExpenseEventKind::Reopened)),
            "EXPENSE_REOPENED",
            true,
            true,
            none(),
        ),
        (
            Box::new(expense_event(ExpenseEventKind::Reimbursed)),
            "EXPENSE_REIMBURSED",
            true,
            true,
            none(),
        ),
        (
            Box::new(receipt_event(ReceiptEventKind::Uploaded { actor_id: user })),
            "RECEIPT_UPLOADED",
            true,
            true,
            on_expense(),
        ),
        (
            Box::new(receipt_event(ReceiptEventKind::Accepted)),
            "RECEIPT_ACCEPTED",
            true,
            false,
            on_expense(),
        ),
        (
            Box::new(receipt_event(ReceiptEventKind::Quarantined)),
            "RECEIPT_QUARANTINED",
            true,
            false,
            on_expense(),
        ),
        (
            Box::new(receipt_event(ReceiptEventKind::Deleted { actor_id: user })),
            "RECEIPT_DELETED",
            true,
            true,
            on_expense(),
        ),
        (
            Box::new(ImportCompleted {
                event_id: id(),
                team_id: team,
                actor_id: user,
                import_id: ImportId::new_v4(),
                imported_rows: 3,
                skipped_rows: 1,
                failed: false,
                occurred_at: at,
            }),
            "IMPORT_COMPLETED",
            true,
            true,
            serde_json::json!({ "importedRows": 3, "skippedRows": 1, "failed": false }),
        ),
    ]
}

fn listener(database: &TestDatabase) -> AuditListener {
    AuditListener {
        pool: database.pool().clone(),
        clock: Arc::new(FixedClock::at(now())),
    }
}

#[derive(Debug, sqlx::FromRow)]
struct Stored {
    team_id: Option<Uuid>,
    actor_id: Option<Uuid>,
    event_type: String,
    details: Value,
    occurred_at: chrono::DateTime<Utc>,
    created_at: chrono::DateTime<Utc>,
}

/// Java: `EveryEventReachesTheAuditTrailTest`: all 25 types, Java's names, team and actor where Java has them,
/// ids and field names only, and a second delivery adds nothing.
#[tokio::test]
async fn every_event_reaches_the_audit_trail_once_with_javas_name_and_details() {
    let database = TestDatabase::new().await.unwrap();
    let listener = listener(&database);
    let (team, user) = (TeamId::new_v7(), UserId::new_v7());
    let samples = samples(team, user);
    assert_eq!(samples.len(), AUDITED_EVENT_TYPES.len());

    for (event, java_type, has_team, has_actor, details) in samples {
        assert!(listener.listens_to(event.event_type()), "{}", event.event_type());
        let payload = event.payload().unwrap();
        listener.deliver(event.event_type(), &payload).await.unwrap();
        listener.deliver(event.event_type(), &payload).await.unwrap();

        let rows: Vec<Stored> = sqlx::query_as(
            "select team_id, actor_id, event_type, details, occurred_at, created_at from audit_events \
             where event_id = $1",
        )
        .bind(event.event_id())
        .fetch_all(database.pool())
        .await
        .unwrap();
        assert_eq!(rows.len(), 1, "{java_type}");
        let row = &rows[0];
        assert_eq!(row.event_type, java_type);
        assert_eq!(row.team_id, has_team.then(|| team.as_uuid()), "{java_type}");
        assert_eq!(row.actor_id, has_actor.then(|| user.as_uuid()), "{java_type}");
        assert_eq!(row.details, details, "{java_type}");
        assert_eq!((row.occurred_at, row.created_at), (now(), now()));
    }
}

#[test]
fn every_audited_type_is_a_type_some_module_publishes_and_nothing_else_is_audited() {
    let samples = samples(TeamId::new_v7(), UserId::new_v7());
    let published: std::collections::BTreeSet<&str> = samples.iter().map(|sample| sample.0.event_type()).collect();
    let audited: std::collections::BTreeSet<&str> = AUDITED_EVENT_TYPES.into_iter().collect();
    assert_eq!(published, audited);
    // The import job's own start signal is not an action anyone took.
    assert!(!AUDITED_EVENT_TYPES.contains(&"ledgerly-rust:imports.ImportQueued"));
}

/// Java: `AuditTrailTest`: the database refuses changes, for the app's role as for a superuser (privileges,
/// then triggers).
#[tokio::test]
async fn the_trail_cannot_be_updated_deleted_or_truncated() {
    let database = TestDatabase::new().await.unwrap();
    let (team, user) = (TeamId::new_v7(), UserId::new_v7());
    let (event, ..) = samples(team, user).swap_remove(3);
    listener(&database)
        .deliver(event.event_type(), &event.payload().unwrap())
        .await
        .unwrap();

    for statement in [
        "update audit_events set event_type = 'FORGED'",
        "delete from audit_events",
        "truncate audit_events",
    ] {
        let error = sqlx::query(statement).execute(database.pool()).await.unwrap_err();
        assert!(
            error.to_string().contains("append-only") || error.to_string().contains("permission denied"),
            "{statement}: {error}"
        );
    }
    let count: i64 = sqlx::query_scalar("select count(*) from audit_events where event_type = 'TEAM_CREATED'")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

/// Java: `AuditTrailTest`: rows without a team never show in a team's page, even the team of that very user.
#[tokio::test]
async fn team_less_rows_are_never_part_of_a_teams_page() {
    let database = TestDatabase::new().await.unwrap();
    let listener = listener(&database);
    let (team, user) = (TeamId::new_v7(), UserId::new_v7());
    for (event, ..) in samples(team, user) {
        listener
            .deliver(event.event_type(), &event.payload().unwrap())
            .await
            .unwrap();
    }
    let mut connection = database.pool().acquire().await.unwrap();

    let page = trail::page(&mut connection, team, None, 100).await.unwrap();

    assert_eq!(page.items.len(), 22);
    assert!(
        page.items
            .iter()
            .all(|row| !row.event_type.starts_with("USER_") && row.event_type != "REFRESH_TOKEN_REUSE_DETECTED")
    );
    let import = page
        .items
        .iter()
        .find(|row| row.event_type == "IMPORT_COMPLETED")
        .unwrap();
    // As Postgres prints the jsonb, which is how Java reads it back.
    assert_eq!(
        serde_json::to_string(&import.details.0).unwrap(),
        r#"{"failed":false,"skippedRows":1,"importedRows":3}"#
    );
}
