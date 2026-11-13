//! Teams, memberships, roles, invitations and `/me`.
//!
//! Java: `src/test/java/com/ledgerly/teams/internal/web/TeamsApiTest.java`.
//!
//! Not here, because they need a clock (see `NOT_PARITY.md`): an invitation expiring after 7 days, in
//! `:accept` and in `/me`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::http::Response;
use ledgerly_parity_tests::{Session, V1, World, expense_json};
use serde_json::{Value, json};
use uuid::Uuid;

/// Java: `TeamsApiTest.Team`: a team created through the API by a fresh verified user, who is its OWNER.
struct Team {
    id: Uuid,
    owner: Session,
}

impl Team {
    fn path(&self) -> String {
        format!("{V1}/teams/{}", self.id)
    }
}

async fn new_team(world: &World) -> Team {
    let owner = world.user().await;
    let response = world
        .as_user(&owner)
        .post(&format!("{V1}/teams"))
        .json(&json!({"name": "Acme"}))
        .send()
        .await;
    assert_eq!(response.status, 201);
    Team {
        id: id_of(&response).parse().unwrap(),
        owner,
    }
}

fn id_of(response: &Response) -> String {
    response.json()["id"]
        .as_str()
        .expect("the response has an id")
        .to_owned()
}

/// A teammate added with SQL, as Java's `TeamSeeder.member` does.
async fn teammate(world: &World, team: &Team, role: &str) -> Session {
    let user = world.user().await;
    world.fixtures.member(team.id, &user.user, role).await;
    user
}

async fn invite(world: &World, team: &Team, email: &str, role: &str) -> Response {
    world
        .as_user(&team.owner)
        .post(&format!("{}/invitations", team.path()))
        .json(&json!({"email": email, "role": role}))
        .send()
        .await
}

/// Java: `invitation()`: an invitation that was created.
async fn invitation(world: &World, team: &Team, email: &str, role: &str) -> Response {
    let response = invite(world, team, email, role).await;
    assert_eq!(response.status, 201);
    response
}

async fn accept(world: &World, user: &Session, invitation_id: &str) -> Response {
    world
        .as_user(user)
        .post(&format!("{V1}/invitations/{invitation_id}:accept"))
        .send()
        .await
}

async fn decline(world: &World, user: &Session, invitation_id: &str) -> Response {
    world
        .as_user(user)
        .post(&format!("{V1}/invitations/{invitation_id}:decline"))
        .send()
        .await
}

async fn me(world: &World, user: &Session) -> Value {
    world.as_user(user).get(&format!("{V1}/me")).send().await.json()
}

/// Java: `roleIn()`: the caller's role in `team` according to `/me`, or `None`.
async fn role_in(world: &World, user: &Session, team: Uuid) -> Option<String> {
    me(world, user).await["teams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["teamId"] == team.to_string())
        .map(|entry| entry["role"].as_str().unwrap().to_owned())
}

mod m6 {
    use super::*;

    // --- creating a team ---

    /// Java: `TeamsApiTest.creatorBecomesOwnerAndGetsALocation`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn creator_becomes_owner_and_gets_a_location() {
        let world = World::start().await;
        let user = world.user().await;

        let response = world
            .as_user(&user)
            .post(&format!("{V1}/teams"))
            .json(&json!({"name": "Acme"}))
            .send()
            .await;

        assert_eq!(response.status, 201);
        let id = id_of(&response);
        // Java compares the path of the Location, whatever host it names.
        let location = response.header("location").unwrap();
        assert!(location.ends_with(&format!("/api/v1/teams/{id}")), "{location}");
        let path = location
            .split_once("://")
            .map_or(location, |(_, rest)| &rest[rest.find('/').unwrap()..]);
        assert_eq!(path, format!("/api/v1/teams/{id}"));
        assert_eq!(
            role_in(&world, &user, id.parse().unwrap()).await.as_deref(),
            Some("OWNER")
        );
    }

    /// Java: `TeamsApiTest.teamNameIsValidated`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn team_name_is_validated() {
        let world = World::start().await;
        let user = world.as_user(&world.user().await);
        let teams = format!("{V1}/teams");

        assert_eq!(user.post(&teams).json(&json!({"name": ""})).send().await.status, 400);
        assert_eq!(
            user.post(&teams)
                .json(&json!({"name": "x".repeat(101)}))
                .send()
                .await
                .status,
            400
        );
        assert_eq!(user.post(&teams).json(&json!({})).send().await.status, 400);
        assert_eq!(
            world
                .anonymous()
                .post(&teams)
                .json(&json!({"name": "A"}))
                .send()
                .await
                .status,
            401
        );
    }

    // --- inviting (API6) ---

    /// Java: `TeamsApiTest.invitingARegisteredAndAnUnknownEmailLooksIdentical`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn inviting_a_registered_and_an_unknown_email_looks_identical() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let registered = world.fixtures.user().await;

        let known = invitation(&world, &team, &registered.email, "MEMBER").await;
        let unknown = invitation(
            &world,
            &team,
            &format!("nobody-{}@example.com", Uuid::new_v4()),
            "MEMBER",
        )
        .await;

        assert_eq!(known.status, unknown.status);
        assert_eq!(known.property_names(), unknown.property_names());
        assert_eq!(known.json()["status"], unknown.json()["status"]);
        assert_eq!(known.json()["role"], unknown.json()["role"]);
    }

    /// Java: `TeamsApiTest.invitingAnExistingTeammateLooksIdentical`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn inviting_an_existing_teammate_looks_identical() {
        let world = World::start().await;
        let team = new_team(&world).await;

        // The owner's own address is already a teammate; the answer must not reveal that.
        let own = invitation(&world, &team, team.owner.email(), "MEMBER").await;
        let other = invitation(
            &world,
            &team,
            &format!("other-{}@example.com", Uuid::new_v4()),
            "MEMBER",
        )
        .await;

        assert_eq!(own.status, other.status);
        assert_eq!(own.property_names(), other.property_names());
    }

    /// Java: `TeamsApiTest.invitationInputIsValidated`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn invitation_input_is_validated() {
        let world = World::start().await;
        let team = new_team(&world).await;

        assert_eq!(invite(&world, &team, "not-an-email", "MEMBER").await.status, 400);
        assert_eq!(invite(&world, &team, "a@example.com", "BOSS").await.status, 400);
    }

    /// Java: `TeamsApiTest.reInvitingReplacesThePendingInvitation`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn re_inviting_replaces_the_pending_invitation() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.user().await;

        let first = invitation(&world, &team, invitee.email(), "MEMBER").await;
        let second = invitation(&world, &team, &invitee.email().to_uppercase(), "APPROVER").await;

        assert_ne!(id_of(&second), id_of(&first));
        let pending: i64 =
            sqlx::query_scalar("select count(*) from invitations where team_id = $1 and status = 'PENDING'")
                .bind(team.id)
                .fetch_one(world.fixtures.pool())
                .await
                .unwrap();
        assert_eq!(pending, 1);
        assert_eq!(accept(&world, &invitee, &id_of(&first)).await.status, 409);
        let accepted = accept(&world, &invitee, &id_of(&second)).await;
        assert_eq!(accepted.status, 200);
        assert_eq!(accepted.json()["role"], "APPROVER");
    }

    // --- accepting and declining (API2, API6) ---

    /// Java: `TeamsApiTest.verifiedInviteeAcceptsAndJoinsWithTheInvitedRole`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn verified_invitee_accepts_and_joins_with_the_invited_role() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.user().await;
        let invitation = invitation(&world, &team, invitee.email(), "APPROVER").await;

        let response = accept(&world, &invitee, &id_of(&invitation)).await;

        assert_eq!(response.status, 200);
        assert_eq!(response.json()["teamId"], team.id.to_string());
        assert_eq!(role_in(&world, &invitee, team.id).await.as_deref(), Some("APPROVER"));
    }

    /// Java: `TeamsApiTest.emailMatchingIgnoresCase`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn email_matching_ignores_case() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.user().await;

        let invitation = invitation(&world, &team, &invitee.email().to_uppercase(), "MEMBER").await;

        assert_eq!(accept(&world, &invitee, &id_of(&invitation)).await.status, 200);
    }

    /// Java: `TeamsApiTest.unverifiedUserCannotAcceptAnInvitation_API2`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn unverified_user_cannot_accept_an_invitation() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.unverified_user().await;
        let invitation = invitation(&world, &team, invitee.email(), "MEMBER").await;

        let response = accept(&world, &invitee, &id_of(&invitation)).await;

        assert_eq!(response.status, 403);
        assert_eq!(response.problem_code().as_deref(), Some("EMAIL_NOT_VERIFIED"));
        assert_eq!(role_in(&world, &invitee, team.id).await, None);
    }

    /// Java: `TeamsApiTest.someoneElseGets404ForAnInvitationNotAddressedToThem`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn someone_else_gets_404_for_an_invitation_not_addressed_to_them() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.user().await;
        let invitation = id_of(&invitation(&world, &team, invitee.email(), "MEMBER").await);
        let stranger = world.user().await;

        assert_eq!(accept(&world, &stranger, &invitation).await.status, 404);
        assert_eq!(decline(&world, &stranger, &invitation).await.status, 404);
        assert_eq!(accept(&world, &invitee, &Uuid::new_v4().to_string()).await.status, 404);
    }

    /// Java: `TeamsApiTest.anInvitationCanBeAcceptedOnlyOnce`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn an_invitation_can_be_accepted_only_once() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.user().await;
        let invitation = id_of(&invitation(&world, &team, invitee.email(), "MEMBER").await);
        assert_eq!(accept(&world, &invitee, &invitation).await.status, 200);

        let again = accept(&world, &invitee, &invitation).await;

        assert_eq!(again.status, 409);
        assert_eq!(again.problem_code().as_deref(), Some("INVITATION_NOT_PENDING"));
    }

    /// Java: `TeamsApiTest.acceptingWhileAlreadyATeammateIsRefused`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn accepting_while_already_a_teammate_is_refused() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitation = id_of(&invitation(&world, &team, team.owner.email(), "MEMBER").await);

        let response = accept(&world, &team.owner, &invitation).await;

        assert_eq!(response.status, 409);
        assert_eq!(response.problem_code().as_deref(), Some("ALREADY_MEMBER"));
        assert_eq!(role_in(&world, &team.owner, team.id).await.as_deref(), Some("OWNER"));
    }

    /// Java: `TeamsApiTest.declineClosesTheInvitation`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn decline_closes_the_invitation() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.user().await;
        let invitation = id_of(&invitation(&world, &team, invitee.email(), "MEMBER").await);

        assert_eq!(decline(&world, &invitee, &invitation).await.status, 204);
        assert_eq!(accept(&world, &invitee, &invitation).await.status, 409);
        assert_eq!(decline(&world, &invitee, &invitation).await.status, 409);
    }

    /// Java: `TeamsApiTest.revokeClosesTheInvitation`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn revoke_closes_the_invitation() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.user().await;
        let invitation = id_of(&invitation(&world, &team, invitee.email(), "MEMBER").await);
        let path = format!("{}/invitations/{invitation}", team.path());
        let owner = world.as_user(&team.owner);

        assert_eq!(owner.delete(&path).send().await.status, 204);
        assert_eq!(accept(&world, &invitee, &invitation).await.status, 409);
        assert_eq!(owner.delete(&path).send().await.status, 409);
    }

    /// Java: `TeamsApiTest.anotherTeamsInvitationCannotBeRevokedThroughMyTeam_API1`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn another_teams_invitation_cannot_be_revoked_through_my_team() {
        let world = World::start().await;
        let (mine, theirs) = (new_team(&world).await, new_team(&world).await);
        let invitation =
            id_of(&invitation(&world, &theirs, &format!("x-{}@example.com", Uuid::new_v4()), "MEMBER").await);

        let response = world
            .as_user(&mine.owner)
            .delete(&format!("{}/invitations/{invitation}", mine.path()))
            .send()
            .await;

        assert_eq!(response.status, 404);
    }

    // --- role ladder and the last-owner rule ---

    /// Java: `TeamsApiTest.ownerChangesARoleAndTheTargetMustBeATeammate`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn owner_changes_a_role_and_the_target_must_be_a_teammate() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let member = teammate(&world, &team, "MEMBER").await;
        let owner = world.as_user(&team.owner);
        let path = |user: Uuid| format!("{}/members/{user}", team.path());

        let changed = owner
            .patch(&path(member.id()))
            .json(&json!({"role": "APPROVER"}))
            .send()
            .await;
        assert_eq!(changed.status, 200);
        assert_eq!(role_in(&world, &member, team.id).await.as_deref(), Some("APPROVER"));

        let unknown = owner
            .patch(&path(Uuid::new_v4()))
            .json(&json!({"role": "APPROVER"}))
            .send()
            .await;
        let bad_role = owner
            .patch(&path(member.id()))
            .json(&json!({"role": "BOSS"}))
            .send()
            .await;
        assert_eq!(unknown.status, 404);
        assert_eq!(bad_role.status, 400);
    }

    /// Java: `TeamsApiTest.theLastOwnerCannotBeDemotedRemovedOrLeave`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn the_last_owner_cannot_be_demoted_removed_or_leave() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let owner = world.as_user(&team.owner);
        let own = format!("{}/members/{}", team.path(), team.owner.id());

        let demote = owner.patch(&own).json(&json!({"role": "MEMBER"})).send().await;
        let remove = owner.delete(&own).send().await;
        let leave = owner.post(&format!("{}:leave", team.path())).send().await;

        assert_eq!(demote.status, 409);
        assert_eq!(demote.problem_code().as_deref(), Some("LAST_OWNER"));
        assert_eq!(remove.problem_code().as_deref(), Some("LAST_OWNER"));
        assert_eq!(leave.problem_code().as_deref(), Some("LAST_OWNER"));
        assert_eq!(role_in(&world, &team.owner, team.id).await.as_deref(), Some("OWNER"));
    }

    /// Java: `TeamsApiTest.withASecondOwnerTheFirstMayDemoteOrLeave`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn with_a_second_owner_the_first_may_demote_or_leave() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let second = teammate(&world, &team, "OWNER").await;
        let owner = world.as_user(&team.owner);

        let demote = owner
            .patch(&format!("{}/members/{}", team.path(), second.id()))
            .json(&json!({"role": "MEMBER"}))
            .send()
            .await;
        assert_eq!(demote.status, 200);

        world.fixtures.member(team.id, &second.user, "OWNER").await;
        let leave = owner.post(&format!("{}:leave", team.path())).send().await;
        assert_eq!(leave.status, 204);
        assert_eq!(role_in(&world, &second, team.id).await.as_deref(), Some("OWNER"));
    }

    /// Java: `TeamsApiTest.ownerRemovesAMemberWhoThenLosesAccess`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn owner_removes_a_member_who_then_loses_access() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let member = teammate(&world, &team, "MEMBER").await;
        let list = format!("{}/expenses", team.path());
        assert_eq!(world.as_user(&member).get(&list).send().await.status, 200);

        let removed = world
            .as_user(&team.owner)
            .delete(&format!("{}/members/{}", team.path(), member.id()))
            .send()
            .await;

        assert_eq!(removed.status, 204);
        assert_eq!(world.as_user(&member).get(&list).send().await.status, 404);
    }

    /// Java: `TeamsApiTest.aMemberCanLeave`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn a_member_can_leave() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let member = teammate(&world, &team, "MEMBER").await;
        let leave = format!("{}:leave", team.path());

        assert_eq!(world.as_user(&member).post(&leave).send().await.status, 204);
        assert_eq!(role_in(&world, &member, team.id).await, None);
        assert_eq!(world.as_user(&member).post(&leave).send().await.status, 404);
    }

    /// Java: `TeamsApiTest.demotionTakesEffectOnTheNextRequestWithTheSameToken`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn demotion_takes_effect_on_the_next_request_with_the_same_token() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let approver = teammate(&world, &team, "APPROVER").await;
        let list = format!("{}/expenses", team.path());
        assert_eq!(world.as_user(&approver).get(&list).send().await.status, 200);

        world
            .as_user(&team.owner)
            .patch(&format!("{}/members/{}", team.path(), approver.id()))
            .json(&json!({"role": "MEMBER"}))
            .send()
            .await;

        // Roles are read from the database on every call, not from the token.
        assert_eq!(role_in(&world, &approver, team.id).await.as_deref(), Some("MEMBER"));
    }

    // --- GET /me (Java's decision D5) ---

    /// Java: `TeamsApiTest.meListsRolesPerTeamAndPendingInvitationsForAVerifiedEmail`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn me_lists_roles_per_team_and_pending_invitations_for_a_verified_email() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.user().await;
        let invitation = id_of(&invitation(&world, &team, invitee.email(), "APPROVER").await);

        let before = world.as_user(&invitee).get(&format!("{V1}/me")).send().await;
        assert_eq!(before.status, 200);
        let before = before.json();
        assert_eq!(before["email"], invitee.email());
        assert_eq!(before["verified"], true);
        assert_eq!(before["teams"], json!([]));
        assert_eq!(before["invitations"].as_array().unwrap().len(), 1);
        let pending = &before["invitations"][0];
        assert_eq!(pending["id"], invitation);
        assert_eq!(pending["teamName"], "Acme");
        assert_eq!(pending["role"], "APPROVER");

        accept(&world, &invitee, &invitation).await;

        let after = me(&world, &invitee).await;
        assert_eq!(after["invitations"], json!([]));
        assert_eq!(after["teams"][0]["role"], "APPROVER");
    }

    /// Java: `TeamsApiTest.meHidesInvitationsFromAnUnverifiedEmail`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn me_hides_invitations_from_an_unverified_email() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let unverified = world.unverified_user().await;
        invitation(&world, &team, unverified.email(), "MEMBER").await;

        let me = me(&world, &unverified).await;

        assert_eq!(me["verified"], false);
        assert_eq!(me["invitations"], json!([]));
    }

    /// Java: `TeamsApiTest.meOmitsExpiredAndRevokedInvitations`, the revoked half. The expired half needs a
    /// clock.
    #[tokio::test]
    #[ignore = "M6"]
    async fn me_omits_revoked_invitations() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let invitee = world.user().await;
        let invitation = id_of(&invitation(&world, &team, invitee.email(), "MEMBER").await);

        world
            .as_user(&team.owner)
            .delete(&format!("{}/invitations/{invitation}", team.path()))
            .send()
            .await;

        assert_eq!(me(&world, &invitee).await["invitations"], json!([]));
    }

    /// Java: `TeamsApiTest.meNeedsAToken`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn me_needs_a_token() {
        let world = World::start().await;

        assert_eq!(world.anonymous().get(&format!("{V1}/me")).send().await.status, 401);
    }

    // --- edge case: a removed claimant (Java's plan, M6) ---

    /// Java: `TeamsApiTest.removedClaimantCannotWithdrawOrReopenButApproversStillDecide`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn removed_claimant_cannot_withdraw_or_reopen_but_approvers_still_decide() {
        let world = World::start().await;
        let team = new_team(&world).await;
        let claimant = teammate(&world, &team, "MEMBER").await;
        let approver = teammate(&world, &team, "APPROVER").await;
        let base = format!("{}/expenses", team.path());
        let created = world
            .as_user(&claimant)
            .post(&base)
            .json(&expense_json("10.00", "Cafe"))
            .send()
            .await;
        assert_eq!(created.status, 201);
        let id = id_of(&created);
        let expense = format!("{base}/{id}");
        world
            .fixtures
            .accepted_receipt(team.id, id.parse().unwrap(), &claimant.user)
            .await;
        assert_eq!(
            world
                .as_user(&claimant)
                .post(&format!("{expense}:submit"))
                .send()
                .await
                .status,
            200
        );

        world
            .as_user(&team.owner)
            .delete(&format!("{}/members/{}", team.path(), claimant.id()))
            .send()
            .await;

        // The claimant has left the team: no more withdrawing, reopening or reading.
        assert_eq!(
            world
                .as_user(&claimant)
                .post(&format!("{expense}:withdraw"))
                .send()
                .await
                .status,
            404
        );
        assert_eq!(world.as_user(&claimant).get(&expense).send().await.status, 404);
        // Approvers can still decide on the expense, and it keeps its claimant.
        let approved = world
            .as_user(&approver)
            .post(&format!("{expense}:approve"))
            .send()
            .await;
        assert_eq!(approved.status, 200);
        assert_eq!(approved.json()["claimantId"], claimant.id().to_string());
    }
}
