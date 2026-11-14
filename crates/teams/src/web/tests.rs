//! The teams endpoints in-process (axum `oneshot`) against real Postgres, with a clock the tests move.
//!
//! Java: `TeamsApiTest`, including the cases that need a clock and so are not in the black-box parity suite:
//! an invitation expiring after 7 days, in `:accept` and in `/me`.

use std::sync::Arc;

use axum::Router;
use axum::body::Body as HttpBody;
use axum::http::{Method, Request, StatusCode};
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::web::{CurrentUser, render};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::Teams;

struct World {
    database: TestDatabase,
    clock: Arc<FixedClock>,
    router: Router,
}

struct Reply {
    status: StatusCode,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or_default()
    }
}

/// A user inserted with SQL, as Java's tests seed them.
#[derive(Debug, Clone)]
struct User {
    id: UserId,
    email: String,
}

impl World {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let clock = Arc::new(FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 14, 12, 0, 0).unwrap()));
        let shared: Arc<dyn Clock> = clock.clone();
        let teams = Teams::new(database.pool(), &shared);
        let router = Router::new()
            .fallback_service(teams.routes().into_service())
            .layer(axum::middleware::from_fn(render));
        Self {
            database,
            clock,
            router,
        }
    }

    fn pool(&self) -> &PgPool {
        self.database.pool()
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    async fn request(&self, method: Method, path: &str, caller: &User, body: Option<&Value>) -> Reply {
        let mut request = Request::builder().method(method).uri(path);
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let mut request = request
            .body(body.map_or_else(HttpBody::empty, |body| HttpBody::from(body.to_string())))
            .unwrap();
        request.extensions_mut().insert(CurrentUser::new(caller.id));
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 16).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply { status, body }
    }

    async fn me(&self, caller: &User) -> Reply {
        self.request(Method::GET, "/api/v1/me", caller, None).await
    }

    async fn user(&self, verified: bool) -> User {
        let user = User {
            id: UserId::new_v7(),
            email: format!("user-{}@example.com", Uuid::new_v4()),
        };
        sqlx::query("insert into users (id, email, password_hash, verified, created_at) values ($1, $2, null, $3, $4)")
            .bind(user.id.as_uuid())
            .bind(&user.email)
            .bind(verified)
            .bind(self.now())
            .execute(self.pool())
            .await
            .unwrap();
        user
    }

    /// A team with `owner` as its OWNER, inserted with SQL.
    async fn team(&self, name: &str, owner: &User) -> TeamId {
        let team = TeamId::new_v7();
        sqlx::query("insert into teams (id, name, created_at) values ($1, $2, $3)")
            .bind(team.as_uuid())
            .bind(name)
            .bind(self.now())
            .execute(self.pool())
            .await
            .unwrap();
        self.member(team, owner, "OWNER").await;
        team
    }

    async fn member(&self, team: TeamId, user: &User, role: &str) {
        sqlx::query("insert into memberships (team_id, user_id, role, joined_at) values ($1, $2, $3, $4)")
            .bind(team.as_uuid())
            .bind(user.id.as_uuid())
            .bind(role)
            .bind(self.now())
            .execute(self.pool())
            .await
            .unwrap();
        // Joining order is what `/me` lists by.
        self.clock.advance(TimeDelta::seconds(1));
    }

    /// An invitation with this stored status, expiring 7 days from now, inserted with SQL.
    async fn invitation(&self, team: TeamId, email: &str, role: &str, status: &str, invited_by: &User) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query(
            "insert into invitations (id, team_id, email, role, status, invited_by, created_at, expires_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id)
        .bind(team.as_uuid())
        .bind(email)
        .bind(role)
        .bind(status)
        .bind(invited_by.id.as_uuid())
        .bind(self.now())
        .bind(self.now() + TimeDelta::days(7))
        .execute(self.pool())
        .await
        .unwrap();
        self.clock.advance(TimeDelta::seconds(1));
        id
    }
}

// --- GET /me ---

#[tokio::test]
async fn me_lists_the_account_its_roles_per_team_and_pending_invitations() {
    let world = World::new().await;
    let user = world.user(true).await;
    let other = world.user(true).await;
    let first = world.team("Acme", &user).await;
    let second = world.team("Bolt", &other).await;
    world.member(second, &user, "APPROVER").await;
    let inviting = world.team("Crux", &other).await;
    let expires_at = world.now() + TimeDelta::days(7);
    let invitation = world
        .invitation(inviting, &user.email, "MEMBER", "PENDING", &other)
        .await;

    let reply = world.me(&user).await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.body,
        json!({
            "id": user.id.to_string(),
            "email": user.email,
            "verified": true,
            "teams": [
                {"teamId": first.to_string(), "name": "Acme", "role": "OWNER"},
                {"teamId": second.to_string(), "name": "Bolt", "role": "APPROVER"},
            ],
            "invitations": [{
                "id": invitation.to_string(),
                "teamId": inviting.to_string(),
                "teamName": "Crux",
                "role": "MEMBER",
                "expiresAt": ledgerly_shared::iso::format_instant(expires_at),
            }],
        })
    );
}

#[tokio::test]
async fn me_matches_invitations_by_the_normalised_email() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.team("Acme", &owner).await;
    let user = world.user(true).await;
    sqlx::query("update users set email = upper($2) where id = $1")
        .bind(user.id.as_uuid())
        .bind(&user.email)
        .execute(world.pool())
        .await
        .unwrap();
    world.invitation(team, &user.email, "MEMBER", "PENDING", &owner).await;

    let reply = world.me(&user).await;

    assert_eq!(reply.body["invitations"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn me_hides_invitations_from_an_unverified_email() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.team("Acme", &owner).await;
    let unverified = world.user(false).await;
    world
        .invitation(team, &unverified.email, "MEMBER", "PENDING", &owner)
        .await;

    let reply = world.me(&unverified).await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["verified"], false);
    assert_eq!(reply.body["invitations"], json!([]));
}

/// Java: `TeamsApiTest.meOmitsExpiredAndRevokedInvitations`.
#[tokio::test]
async fn me_omits_expired_and_answered_invitations() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let user = world.user(true).await;
    for status in ["ACCEPTED", "DECLINED", "REVOKED"] {
        let team = world.team("Answered", &owner).await;
        world.invitation(team, &user.email, "MEMBER", status, &owner).await;
    }
    let team = world.team("Acme", &owner).await;
    world.invitation(team, &user.email, "MEMBER", "PENDING", &owner).await;
    assert_eq!(world.me(&user).await.body["invitations"].as_array().unwrap().len(), 1);

    // Seven days after it was sent, to the microsecond, the invitation has expired.
    world.clock.advance(TimeDelta::days(7) - TimeDelta::seconds(1));

    assert_eq!(world.me(&user).await.body["invitations"], json!([]));
}

#[tokio::test]
async fn me_for_an_account_that_no_longer_exists_is_401() {
    let world = World::new().await;
    let ghost = User {
        id: UserId::new_v7(),
        email: "ghost@example.com".to_owned(),
    };

    let reply = world.me(&ghost).await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reply.code(), "UNAUTHENTICATED");
}
