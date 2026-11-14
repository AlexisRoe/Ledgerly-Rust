//! The teams endpoints in-process (axum `oneshot`) against real Postgres, with a clock the tests move.
//!
//! Java: `TeamsApiTest`, including the cases that need a clock and so are not in the black-box parity suite:
//! an invitation expiring after 7 days, in `:accept` and in `/me`.

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body as HttpBody;
use axum::http::{Method, Request, StatusCode};
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ledgerly_shared::config::TeamsConfig;
use ledgerly_shared::database::TestDatabase;
use ledgerly_shared::events::{Event, Publisher};
use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::web::{CurrentUser, render};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use crate::Teams;

/// Keeps the type of every event, in order. It sees an event when the use case publishes it, before its
/// transaction commits.
#[derive(Debug, Default)]
struct Recorded(Mutex<Vec<&'static str>>);

impl Publisher for Recorded {
    fn publish<'a>(
        &'a self,
        _connection: &'a mut sqlx::PgConnection,
        event: &'a dyn Event,
    ) -> Pin<Box<dyn Future<Output = sqlx::Result<()>> + Send + 'a>> {
        self.0.lock().unwrap().push(event.event_type());
        Box::pin(std::future::ready(Ok(())))
    }
}

struct World {
    database: TestDatabase,
    clock: Arc<FixedClock>,
    router: Router,
    events: Arc<Recorded>,
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
        let events = Arc::new(Recorded::default());
        let publisher: Arc<dyn Publisher> = events.clone();
        let teams = Teams::new(&TeamsConfig::default(), database.pool(), &shared, &publisher);
        let router = Router::new()
            .fallback_service(teams.routes().into_service())
            .layer(axum::middleware::from_fn(render));
        Self {
            database,
            clock,
            router,
            events,
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

    fn events(&self) -> Vec<&'static str> {
        self.events.0.lock().unwrap().clone()
    }

    async fn create_team(&self, caller: &User, name: &str) -> TeamId {
        let reply = self
            .request(Method::POST, "/api/v1/teams", caller, Some(&json!({"name": name})))
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        reply.body["id"].as_str().unwrap().parse().unwrap()
    }

    async fn invite(&self, team: TeamId, caller: &User, email: &str, role: &str) -> Reply {
        let body = json!({"email": email, "role": role});
        self.request(
            Method::POST,
            &format!("/api/v1/teams/{team}/invitations"),
            caller,
            Some(&body),
        )
        .await
    }

    /// The id of a new invitation.
    async fn invited(&self, team: TeamId, caller: &User, email: &str, role: &str) -> String {
        let reply = self.invite(team, caller, email, role).await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        reply.body["id"].as_str().unwrap().to_owned()
    }

    async fn answer(&self, invitation: &str, caller: &User, answer: &str) -> Reply {
        self.request(
            Method::POST,
            &format!("/api/v1/invitations/{invitation}:{answer}"),
            caller,
            None,
        )
        .await
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

// --- creating, inviting and answering ---

#[tokio::test]
async fn a_created_team_has_its_name_stripped_and_the_creator_as_owner() {
    let world = World::new().await;
    let user = world.user(true).await;

    let reply = world
        .request(Method::POST, "/api/v1/teams", &user, Some(&json!({"name": "  Acme  "})))
        .await;

    assert_eq!(reply.status, StatusCode::CREATED);
    assert_eq!(reply.body["name"], "Acme");
    let me = world.me(&user).await;
    assert_eq!(me.body["teams"][0]["role"], "OWNER");
    assert_eq!(world.events(), ["ledgerly-rust:teams.TeamCreated"]);
}

#[tokio::test]
async fn an_invitation_is_pending_for_seven_days() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;

    let reply = world.invite(team, &owner, "New@Example.COM", "APPROVER").await;

    assert_eq!(reply.status, StatusCode::CREATED);
    assert_eq!(reply.body["role"], "APPROVER");
    assert_eq!(reply.body["status"], "PENDING");
    assert_eq!(reply.body["expiresAt"], "2026-11-21T12:00:00Z");
    let stored: String = sqlx::query_scalar("select email from invitations where team_id = $1")
        .bind(team.as_uuid())
        .fetch_one(world.pool())
        .await
        .unwrap();
    assert_eq!(stored, "new@example.com");
}

/// Java: `TeamsApiTest.anExpiredInvitationCannotBeAccepted`.
#[tokio::test]
async fn an_expired_invitation_cannot_be_accepted_declined_or_revoked() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;
    let invitee = world.user(true).await;
    let invitation = world.invited(team, &owner, &invitee.email, "MEMBER").await;

    world.clock.advance(TimeDelta::days(7));

    let accepted = world.answer(&invitation, &invitee, "accept").await;
    assert_eq!(accepted.status, StatusCode::CONFLICT);
    assert_eq!(accepted.code(), "INVITATION_EXPIRED");
    assert_eq!(accepted.body["detail"], "This invitation has expired");
    assert_eq!(
        world.answer(&invitation, &invitee, "decline").await.code(),
        "INVITATION_EXPIRED"
    );
    let revoked = world
        .request(
            Method::DELETE,
            &format!("/api/v1/teams/{team}/invitations/{invitation}"),
            &owner,
            None,
        )
        .await;
    assert_eq!(revoked.code(), "INVITATION_NOT_PENDING");
    assert_eq!(revoked.body["detail"], "Only a pending invitation can be revoked");
}

#[tokio::test]
async fn an_invitation_one_microsecond_before_expiry_can_be_accepted() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;
    let invitee = world.user(true).await;
    let invitation = world.invited(team, &owner, &invitee.email, "MEMBER").await;

    world.clock.advance(TimeDelta::days(7) - TimeDelta::microseconds(1));

    let accepted = world.answer(&invitation, &invitee, "accept").await;
    assert_eq!(accepted.status, StatusCode::OK);
    assert_eq!(accepted.body, json!({"teamId": team.to_string(), "role": "MEMBER"}));
}

#[tokio::test]
async fn re_inviting_after_expiry_replaces_the_expired_invitation() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;
    let invitee = world.user(true).await;
    let first = world.invited(team, &owner, &invitee.email, "MEMBER").await;
    world.clock.advance(TimeDelta::days(8));

    let second = world.invited(team, &owner, &invitee.email, "MEMBER").await;

    assert_eq!(
        world.answer(&first, &invitee, "accept").await.body["detail"],
        "This invitation is REVOKED"
    );
    assert_eq!(world.answer(&second, &invitee, "accept").await.status, StatusCode::OK);
}

#[tokio::test]
async fn an_answered_invitation_names_its_status() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;
    let invitee = world.user(true).await;
    let invitation = world.invited(team, &owner, &invitee.email, "MEMBER").await;
    assert_eq!(
        world.answer(&invitation, &invitee, "decline").await.status,
        StatusCode::NO_CONTENT
    );

    let again = world.answer(&invitation, &invitee, "accept").await;

    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.code(), "INVITATION_NOT_PENDING");
    assert_eq!(again.body["detail"], "This invitation is DECLINED");
}

#[tokio::test]
async fn an_unverified_invitee_may_decline_but_not_accept() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;
    let invitee = world.user(false).await;
    let invitation = world.invited(team, &owner, &invitee.email, "MEMBER").await;

    let accepted = world.answer(&invitation, &invitee, "accept").await;
    assert_eq!(accepted.status, StatusCode::FORBIDDEN);
    assert_eq!(accepted.code(), "EMAIL_NOT_VERIFIED");
    assert_eq!(accepted.body["detail"], "Verify your email address before accepting");

    assert_eq!(
        world.answer(&invitation, &invitee, "decline").await.status,
        StatusCode::NO_CONTENT
    );
}

// --- authorization, in Spring's order ---

#[tokio::test]
async fn a_body_is_validated_before_the_role_is_checked() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;
    let stranger = world.user(true).await;

    // Spring resolves the arguments, the body included, before `@PreAuthorize` runs.
    assert_eq!(
        world.invite(team, &stranger, "not-an-email", "MEMBER").await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        world.invite(team, &stranger, "a@example.com", "MEMBER").await.status,
        StatusCode::NOT_FOUND
    );
    let bad_team = world
        .request(
            Method::POST,
            "/api/v1/teams/not-a-uuid/invitations",
            &owner,
            Some(&json!({})),
        )
        .await;
    assert_eq!(bad_team.code(), "REQUEST_REJECTED");
}

#[tokio::test]
async fn a_teammate_with_the_wrong_role_gets_javas_403() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;
    let approver = world.user(true).await;
    world.member(team, &approver, "APPROVER").await;

    let reply = world.invite(team, &approver, "a@example.com", "MEMBER").await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.code(), "FORBIDDEN");
    assert_eq!(reply.body["detail"], "Role APPROVER may not do this");
}

#[tokio::test]
async fn the_last_owner_details_are_javas() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;

    let demote = world
        .request(
            Method::PATCH,
            &format!("/api/v1/teams/{team}/members/{}", owner.id),
            &owner,
            Some(&json!({"role": "MEMBER"})),
        )
        .await;
    let leave = world
        .request(Method::POST, &format!("/api/v1/teams/{team}:leave"), &owner, None)
        .await;

    assert_eq!(demote.body["detail"], "A team must keep at least one owner");
    assert_eq!(
        leave.body["detail"],
        "The last owner cannot leave; promote someone else first"
    );
}

#[tokio::test]
async fn every_change_publishes_its_event() {
    let world = World::new().await;
    let owner = world.user(true).await;
    let team = world.create_team(&owner, "Acme").await;
    let invitee = world.user(true).await;
    let revoked = world.invited(team, &owner, "gone@example.com", "MEMBER").await;
    world
        .request(
            Method::DELETE,
            &format!("/api/v1/teams/{team}/invitations/{revoked}"),
            &owner,
            None,
        )
        .await;
    let declined = world.invited(team, &owner, &invitee.email, "MEMBER").await;
    world.answer(&declined, &invitee, "decline").await;
    let accepted = world.invited(team, &owner, &invitee.email, "MEMBER").await;
    world.answer(&accepted, &invitee, "accept").await;
    let member = format!("/api/v1/teams/{team}/members/{}", invitee.id);
    world
        .request(Method::PATCH, &member, &owner, Some(&json!({"role": "OWNER"})))
        .await;
    world
        .request(Method::POST, &format!("/api/v1/teams/{team}:leave"), &invitee, None)
        .await;
    let other = world.user(true).await;
    world.member(team, &other, "MEMBER").await;
    world
        .request(
            Method::DELETE,
            &format!("/api/v1/teams/{team}/members/{}", other.id),
            &owner,
            None,
        )
        .await;

    assert_eq!(
        world.events(),
        [
            "ledgerly-rust:teams.TeamCreated",
            "ledgerly-rust:teams.MemberInvited",
            "ledgerly-rust:teams.InvitationRevoked",
            "ledgerly-rust:teams.MemberInvited",
            "ledgerly-rust:teams.InvitationDeclined",
            "ledgerly-rust:teams.MemberInvited",
            "ledgerly-rust:teams.InvitationAccepted",
            "ledgerly-rust:teams.MemberRoleChanged",
            "ledgerly-rust:teams.MemberLeft",
            "ledgerly-rust:teams.MemberRemoved",
        ]
    );
}
