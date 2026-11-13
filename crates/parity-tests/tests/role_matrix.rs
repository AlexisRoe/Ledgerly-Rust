//! Who may call what: every team-scoped endpoint, called without a token, by a non-member, and by a MEMBER, an
//! APPROVER and an OWNER of the team. 16 endpoints × 5 callers = 80 cells.
//!
//! Java: `src/test/java/com/ledgerly/security/RoleMatrixTest.java`. The expected statuses are its table,
//! unchanged. The order of the checks shows in it: not a teammate is 404 everywhere; a MEMBER does not see a
//! colleague's expense (404); an APPROVER or OWNER sees it but may not act as its claimant (403).

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::fixtures::User;
use ledgerly_parity_tests::{V1, World};
use reqwest::Method;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    NoToken,
    NonMember,
    Member,
    Approver,
    Owner,
}

const CALLERS: [Who; 5] = [Who::NoToken, Who::NonMember, Who::Member, Who::Approver, Who::Owner];

/// One team with a colleague who claims the expense under test, and a second OWNER so that leaving is possible.
struct Context {
    team: Uuid,
    member: User,
    approver: User,
    owner: User,
    non_member: User,
    claimant: User,
}

impl Context {
    async fn new(world: &World) -> Self {
        let fixtures = &world.fixtures;
        let context = Self {
            team: fixtures.team().await,
            member: fixtures.user().await,
            approver: fixtures.user().await,
            owner: fixtures.user().await,
            non_member: fixtures.user().await,
            claimant: fixtures.user().await,
        };
        let second_owner = fixtures.user().await;
        fixtures.member(context.team, &context.member, "MEMBER").await;
        fixtures.member(context.team, &context.claimant, "MEMBER").await;
        fixtures.member(context.team, &context.approver, "APPROVER").await;
        fixtures.member(context.team, &context.owner, "OWNER").await;
        fixtures.member(context.team, &second_owner, "OWNER").await;
        context
    }

    fn team_path(&self) -> String {
        format!("{V1}/teams/{}", self.team)
    }

    /// An expense of the claimant (a colleague of every caller) in `status`.
    async fn expense(&self, world: &World, status: &str) -> String {
        let id = world.fixtures.expense(self.team, &self.claimant, status).await;
        format!("{}/expenses/{id}", self.team_path())
    }

    /// A pending invitation, written with SQL as Java's fixture does.
    async fn invitation(&self, world: &World) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "insert into invitations (id, team_id, email, role, status, invited_by, created_at, expires_at) \
             values ($1, $2, $3, 'MEMBER', 'PENDING', $4, now(), now() + interval '7 days')",
        )
        .bind(id)
        .bind(self.team)
        .bind(format!("matrix-{id}@example.com"))
        .bind(self.owner.id)
        .execute(world.fixtures.pool())
        .await
        .unwrap();
        id
    }
}

struct Request {
    method: Method,
    path: String,
    body: Option<Value>,
    if_match: Option<&'static str>,
}

impl Request {
    fn new(method: Method, path: String) -> Self {
        Self {
            method,
            path,
            body: None,
            if_match: None,
        }
    }

    fn body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    fn if_match(mut self, etag: &'static str) -> Self {
        self.if_match = Some(etag);
        self
    }
}

#[derive(Debug, Clone, Copy)]
enum Endpoint {
    CreateExpense,
    ListExpenses,
    GetExpense,
    PatchExpense,
    DeleteExpense,
    Submit,
    Withdraw,
    Approve,
    Reject,
    Reopen,
    Reimburse,
    Invite,
    RevokeInvitation,
    ChangeRole,
    RemoveMember,
    Leave,
}

/// Java: `RoleMatrixTest.ENDPOINTS`.           NO_TOKEN, NON_MEMBER, MEMBER, APPROVER, OWNER
const ENDPOINTS: [(&str, Endpoint, [u16; 5]); 16] = [
    ("POST   expenses", Endpoint::CreateExpense, [401, 404, 201, 201, 201]),
    ("GET    expenses", Endpoint::ListExpenses, [401, 404, 200, 200, 200]),
    ("GET    expenses/{id}", Endpoint::GetExpense, [401, 404, 404, 200, 200]),
    (
        "PATCH  expenses/{id}",
        Endpoint::PatchExpense,
        [401, 404, 404, 403, 403],
    ),
    (
        "DELETE expenses/{id}",
        Endpoint::DeleteExpense,
        [401, 404, 404, 403, 403],
    ),
    (
        "POST   expenses/{id}:submit",
        Endpoint::Submit,
        [401, 404, 404, 403, 403],
    ),
    (
        "POST   expenses/{id}:withdraw",
        Endpoint::Withdraw,
        [401, 404, 404, 403, 403],
    ),
    (
        "POST   expenses/{id}:approve",
        Endpoint::Approve,
        [401, 404, 404, 200, 200],
    ),
    (
        "POST   expenses/{id}:reject",
        Endpoint::Reject,
        [401, 404, 404, 200, 200],
    ),
    (
        "POST   expenses/{id}:reopen",
        Endpoint::Reopen,
        [401, 404, 404, 403, 403],
    ),
    (
        "POST   expenses/{id}:reimburse",
        Endpoint::Reimburse,
        [401, 404, 404, 403, 200],
    ),
    ("POST   invitations", Endpoint::Invite, [401, 404, 403, 403, 201]),
    (
        "DELETE invitations/{id}",
        Endpoint::RevokeInvitation,
        [401, 404, 403, 403, 204],
    ),
    ("PATCH  members/{id}", Endpoint::ChangeRole, [401, 404, 403, 403, 200]),
    ("DELETE members/{id}", Endpoint::RemoveMember, [401, 404, 403, 403, 204]),
    ("POST   :leave", Endpoint::Leave, [401, 404, 204, 204, 204]),
];

impl Endpoint {
    /// Sets up the state the endpoint needs and says what to send.
    async fn prepare(self, world: &World, context: &Context) -> Request {
        let team = context.team_path();
        let claimant = format!("{team}/members/{}", context.claimant.id);
        match self {
            Self::CreateExpense => Request::new(Method::POST, format!("{team}/expenses")).body(json!({
                "money": {"amount": "5.00", "currency": "EUR"},
                "category": "MEALS",
                "incurredOn": ledgerly_parity_tests::today(),
                "merchant": "Cafe",
            })),
            Self::ListExpenses => Request::new(Method::GET, format!("{team}/expenses")),
            Self::GetExpense => Request::new(Method::GET, context.expense(world, "DRAFT").await),
            Self::PatchExpense => Request::new(Method::PATCH, context.expense(world, "DRAFT").await)
                .body(json!({"merchant": "X"}))
                .if_match("\"0\""),
            Self::DeleteExpense => {
                Request::new(Method::DELETE, context.expense(world, "DRAFT").await).if_match("\"0\"")
            }
            Self::Submit => Request::new(
                Method::POST,
                format!("{}:submit", context.expense(world, "DRAFT").await),
            ),
            Self::Withdraw => Request::new(
                Method::POST,
                format!("{}:withdraw", context.expense(world, "SUBMITTED").await),
            ),
            Self::Approve => Request::new(
                Method::POST,
                format!("{}:approve", context.expense(world, "SUBMITTED").await),
            ),
            Self::Reject => Request::new(
                Method::POST,
                format!("{}:reject", context.expense(world, "SUBMITTED").await),
            )
            .body(json!({"reason": "no"})),
            Self::Reopen => Request::new(
                Method::POST,
                format!("{}:reopen", context.expense(world, "REJECTED").await),
            ),
            Self::Reimburse => Request::new(
                Method::POST,
                format!("{}:reimburse", context.expense(world, "APPROVED").await),
            ),
            Self::Invite => Request::new(Method::POST, format!("{team}/invitations"))
                .body(json!({"email": "new@example.com", "role": "MEMBER"})),
            Self::RevokeInvitation => Request::new(
                Method::DELETE,
                format!("{team}/invitations/{}", context.invitation(world).await),
            ),
            Self::ChangeRole => Request::new(Method::PATCH, claimant).body(json!({"role": "APPROVER"})),
            Self::RemoveMember => Request::new(Method::DELETE, claimant),
            Self::Leave => Request::new(Method::POST, format!("{team}:leave")),
        }
    }
}

mod m6 {
    use super::*;

    /// Java: `RoleMatrixTest.everyCellOfTheMatrix`. Every cell gets a fresh team, as in Java; all cells are
    /// tried, and the ones that differ are reported together.
    #[tokio::test]
    #[ignore = "M6"]
    async fn every_cell_of_the_matrix() {
        let world = World::start().await;
        let mut wrong = Vec::new();

        for (name, endpoint, expected) in ENDPOINTS {
            for (who, expected_status) in CALLERS.into_iter().zip(expected) {
                let context = Context::new(&world).await;
                let request = endpoint.prepare(&world, &context).await;
                let client = match who {
                    Who::NoToken => world.anonymous(),
                    Who::NonMember => world.as_user(&world.sign_in(&context.non_member).await),
                    Who::Member => world.as_user(&world.sign_in(&context.member).await),
                    Who::Approver => world.as_user(&world.sign_in(&context.approver).await),
                    Who::Owner => world.as_user(&world.sign_in(&context.owner).await),
                };
                let mut sending = client.request(request.method, &request.path);
                if let Some(etag) = request.if_match {
                    sending = sending.header("If-Match", etag);
                }
                if let Some(body) = &request.body {
                    sending = sending.json(body);
                }

                let status = sending.send().await.status;

                if status != expected_status {
                    wrong.push(format!("{name} as {who:?}: expected {expected_status}, got {status}"));
                }
            }
        }

        assert!(
            wrong.is_empty(),
            "{} of 80 cells differ:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }
}
