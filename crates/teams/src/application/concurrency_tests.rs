//! Two requests change the same thing at the same moment, and exactly one of them may win.
//!
//! Two owners act on each other at the same moment. Without the team row lock both see "two owners left" and
//! both succeed, leaving none. With it they run one after the other, and the second finds that it is no longer
//! allowed.
//!
//! Java: `teams/internal/application/LastOwnerConcurrencyTest.java`, with a `CyclicBarrier` and two threads,
//! each case repeated 10 times. Here: a Tokio barrier and two tasks on a multi-threaded runtime.

use std::sync::Arc;

use chrono::Utc;
use ledgerly_shared::clock::{Clock, SystemClock};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::events::{Discard, Publisher};
use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::role::Role;
use sqlx::PgPool;
use tokio::sync::Barrier;

use ledgerly_shared::id::InvitationId;
use uuid::Uuid;

use super::{
    AcceptInvitation, ChangeMemberRole, Context, DeclineInvitation, LeaveTeam, RemoveMember, RevokeInvitation,
    UseCaseError,
};

const ROUNDS: usize = 10;

fn context(pool: &PgPool) -> Context {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let events: Arc<dyn Publisher> = Arc::new(Discard);
    Context {
        pool: pool.clone(),
        clock,
        events,
    }
}

/// A new team with two OWNERs, inserted with SQL as Java's `TeamSeeder` does.
async fn two_owners(pool: &PgPool) -> (TeamId, UserId, UserId) {
    let (team, a, b) = (TeamId::new_v7(), UserId::new_v7(), UserId::new_v7());
    sqlx::query("insert into teams (id, name, created_at) values ($1, 'Race', $2)")
        .bind(team.as_uuid())
        .bind(Utc::now())
        .execute(pool)
        .await
        .unwrap();
    for user in [a, b] {
        sqlx::query("insert into memberships (team_id, user_id, role, joined_at) values ($1, $2, 'OWNER', $3)")
            .bind(team.as_uuid())
            .bind(user.as_uuid())
            .bind(Utc::now())
            .execute(pool)
            .await
            .unwrap();
    }
    (team, a, b)
}

async fn owners(pool: &PgPool, team: TeamId) -> i64 {
    sqlx::query_scalar("select count(*) from memberships where team_id = $1 and role = 'OWNER'")
        .bind(team.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Starts both actions at the same instant on two worker threads; returns how each one ended.
async fn race<F1, F2>(first: F1, second: F2) -> [Result<(), UseCaseError>; 2]
where
    F1: Future<Output = Result<(), UseCaseError>> + Send + 'static,
    F2: Future<Output = Result<(), UseCaseError>> + Send + 'static,
{
    let start = Arc::new(Barrier::new(2));
    let one = tokio::spawn({
        let start = start.clone();
        async move {
            start.wait().await;
            first.await
        }
    });
    let two = tokio::spawn(async move {
        start.wait().await;
        second.await
    });
    [one.await.unwrap(), two.await.unwrap()]
}

/// Exactly one of the two succeeded, and the other failed only in one of the `allowed` ways.
fn one_winner(outcomes: &[Result<(), UseCaseError>; 2], allowed: fn(&UseCaseError) -> bool) {
    let successes = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    assert_eq!(successes, 1, "{outcomes:?}");
    for outcome in outcomes {
        if let Err(error) = outcome {
            assert!(allowed(error), "{error:?}");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_owners_demoting_each_other_leave_exactly_one_owner() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let change_role = ChangeMemberRole::new(context(pool));
    for _ in 0..ROUNDS {
        let (team, a, b) = two_owners(pool).await;

        let outcomes = race(
            {
                let change_role = change_role.clone();
                async move { change_role.execute(team, a, b, Role::Member).await.map(drop) }
            },
            {
                let change_role = change_role.clone();
                async move { change_role.execute(team, b, a, Role::Member).await.map(drop) }
            },
        )
        .await;

        assert_eq!(owners(pool, team).await, 1);
        // The loser was an owner when it started, but no longer is once it holds the lock.
        one_winner(&outcomes, |error| {
            matches!(error, UseCaseError::Forbidden(_) | UseCaseError::LastOwner(_))
        });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_owners_removing_each_other_leave_exactly_one_owner() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let remove = RemoveMember::new(context(pool));
    for _ in 0..ROUNDS {
        let (team, a, b) = two_owners(pool).await;

        let outcomes = race(
            {
                let remove = remove.clone();
                async move { remove.execute(team, a, b).await }
            },
            {
                let remove = remove.clone();
                async move { remove.execute(team, b, a).await }
            },
        )
        .await;

        assert_eq!(owners(pool, team).await, 1);
        one_winner(&outcomes, |error| {
            matches!(
                error,
                UseCaseError::Forbidden(_) | UseCaseError::NotFound | UseCaseError::LastOwner(_)
            )
        });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_owners_leaving_at_the_same_time_leave_exactly_one_owner() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let leave = LeaveTeam::new(context(pool));
    for _ in 0..ROUNDS {
        let (team, a, b) = two_owners(pool).await;

        let outcomes = race(
            {
                let leave = leave.clone();
                async move { leave.execute(team, a).await }
            },
            {
                let leave = leave.clone();
                async move { leave.execute(team, b).await }
            },
        )
        .await;

        assert_eq!(owners(pool, team).await, 1);
        one_winner(&outcomes, |error| matches!(error, UseCaseError::LastOwner(_)));
    }
}

/// A verified user with a pending invitation to the team of `owner`, inserted with SQL.
async fn invited(pool: &PgPool, team: TeamId, owner: UserId) -> (UserId, InvitationId) {
    let (user, invitation) = (UserId::new_v7(), InvitationId::new_v7());
    let email = format!("race-{}@example.com", Uuid::new_v4());
    sqlx::query("insert into users (id, email, password_hash, verified, created_at) values ($1, $2, null, true, $3)")
        .bind(user.as_uuid())
        .bind(&email)
        .bind(Utc::now())
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "insert into invitations (id, team_id, email, role, status, invited_by, created_at, expires_at) \
         values ($1, $2, $3, 'MEMBER', 'PENDING', $4, now(), now() + interval '7 days')",
    )
    .bind(invitation.as_uuid())
    .bind(team.as_uuid())
    .bind(&email)
    .bind(owner.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    (user, invitation)
}

/// The invitation's stored status and whether its addressee became a member: they must agree.
async fn outcome(pool: &PgPool, team: TeamId, user: UserId, invitation: InvitationId) -> (String, bool) {
    let status: String = sqlx::query_scalar("select status from invitations where id = $1")
        .bind(invitation.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    let member: bool =
        sqlx::query_scalar("select exists (select 1 from memberships where team_id = $1 and user_id = $2)")
            .bind(team.as_uuid())
            .bind(user.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    (status, member)
}

/// Not in Java, which locks no invitation row, so both can succeed there (decided after M6).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepting_and_declining_at_the_same_time_lets_exactly_one_win() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let (accept, decline) = (
        AcceptInvitation::new(context(pool)),
        DeclineInvitation::new(context(pool)),
    );
    for _ in 0..ROUNDS {
        let (team, owner, _) = two_owners(pool).await;
        let (user, invitation) = invited(pool, team, owner).await;

        let outcomes = race(
            {
                let accept = accept.clone();
                async move { accept.execute(invitation, user).await.map(drop) }
            },
            {
                let decline = decline.clone();
                async move { decline.execute(invitation, user).await }
            },
        )
        .await;

        one_winner(&outcomes, |error| {
            matches!(error, UseCaseError::InvitationNotPending(_))
        });
        let accepted = outcomes[0].is_ok();
        let expected = if accepted { "ACCEPTED" } else { "DECLINED" };
        assert_eq!(
            outcome(pool, team, user, invitation).await,
            (expected.to_owned(), accepted)
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepting_and_revoking_at_the_same_time_lets_exactly_one_win() {
    let database = TestDatabase::new().await.unwrap();
    let pool = database.pool();
    let (accept, revoke) = (
        AcceptInvitation::new(context(pool)),
        RevokeInvitation::new(context(pool)),
    );
    for _ in 0..ROUNDS {
        let (team, owner, _) = two_owners(pool).await;
        let (user, invitation) = invited(pool, team, owner).await;

        let outcomes = race(
            {
                let accept = accept.clone();
                async move { accept.execute(invitation, user).await.map(drop) }
            },
            {
                let revoke = revoke.clone();
                async move { revoke.execute(team, invitation, owner).await }
            },
        )
        .await;

        one_winner(&outcomes, |error| {
            matches!(error, UseCaseError::InvitationNotPending(_))
        });
        let accepted = outcomes[0].is_ok();
        let expected = if accepted { "ACCEPTED" } else { "REVOKED" };
        assert_eq!(
            outcome(pool, team, user, invitation).await,
            (expected.to_owned(), accepted)
        );
    }
}
