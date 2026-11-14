//! OWASP API5:2023 Broken Function Level Authorization: the role ladder is checked on every call, from the
//! database, and nothing in a request can raise a caller's role.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api5BrokenFunctionLevelAuthorizationTest.java`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::World;
use serde_json::json;

mod m6 {
    use super::*;

    /// Java: `Api5BrokenFunctionLevelAuthorizationTest.aMemberCallingApproveGets403`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn a_member_calling_approve_gets_403() {
        let world = World::start().await;
        let team = world.team().await;
        let submitted_by_member = world.expense(&team, &team.member, "SUBMITTED").await;

        // The member claimed it, so they can see it; being a member is what stops them deciding.
        let response = world
            .as_user(&team.member)
            .post(&format!("{}:approve", team.expense_path(submitted_by_member)))
            .send()
            .await;

        assert_eq!(response.status, 403);
        assert_eq!(response.problem_code().as_deref(), Some("FORBIDDEN"));
    }

    /// Java: `Api5BrokenFunctionLevelAuthorizationTest.anApproverCannotReimburseAndAMemberCannotReject`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn an_approver_cannot_reimburse_and_a_member_cannot_reject() {
        let world = World::start().await;
        let team = world.team().await;
        let approved = world.expense(&team, &team.member, "APPROVED").await;
        let submitted = world.expense(&team, &team.member, "SUBMITTED").await;

        let reimburse = world
            .as_user(&team.approver)
            .post(&format!("{}:reimburse", team.expense_path(approved)))
            .send()
            .await;
        let reject = world
            .as_user(&team.member)
            .post(&format!("{}:reject", team.expense_path(submitted)))
            .json(&json!({"reason": "no"}))
            .send()
            .await;

        assert_eq!(reimburse.status, 403);
        assert_eq!(reject.status, 403);
    }

    /// Java: `Api5BrokenFunctionLevelAuthorizationTest.nobodyCanActAsAnotherRoleThroughHeadersOrTokens`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn nobody_can_act_as_another_role_through_headers_or_tokens() {
        let world = World::start().await;
        let team = world.team().await;
        let submitted = world.expense(&team, &team.colleague, "SUBMITTED").await;

        // Old debug headers and role claims mean nothing: the role is read from the database.
        let response = world
            .as_user(&team.member)
            .post(&format!("{}:approve", team.expense_path(submitted)))
            .header("X-Debug-Role", "OWNER")
            .header("X-Debug-User-Id", &team.owner.id().to_string())
            .send()
            .await;

        // A member does not even see a colleague's expense.
        assert_eq!(response.status, 404);
    }

    /// Java: `Api5BrokenFunctionLevelAuthorizationTest.aDemotionTakesEffectOnTheNextRequestWithTheSameToken`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn a_demotion_takes_effect_on_the_next_request_with_the_same_token() {
        let world = World::start().await;
        let team = world.team().await;
        let submitted = world.expense(&team, &team.colleague, "SUBMITTED").await;
        let path = team.expense_path(submitted);
        assert_eq!(world.as_user(&team.approver).get(&path).send().await.status, 200);

        world
            .as_user(&team.owner)
            .patch(&format!("{}/members/{}", team.path(), team.approver.id()))
            .json(&json!({"role": "MEMBER"}))
            .send()
            .await;

        assert_eq!(world.as_user(&team.approver).get(&path).send().await.status, 404);
    }
}

mod m9 {
    use super::*;

    /// Java: `Api5BrokenFunctionLevelAuthorizationTest.teamAdministrationIsForOwnersOnly`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn team_administration_is_for_owners_only() {
        let world = World::start().await;
        let team = world.team().await;
        let invite = json!({"email": "someone@example.com", "role": "MEMBER"});
        let invitations = format!("{}/invitations", team.path());
        let colleague = format!("{}/members/{}", team.path(), team.colleague.id());

        for user in [&team.approver, &team.member] {
            let client = world.as_user(user);
            assert_eq!(client.post(&invitations).json(&invite).send().await.status, 403);
            assert_eq!(
                client
                    .patch(&colleague)
                    .json(&json!({"role": "OWNER"}))
                    .send()
                    .await
                    .status,
                403
            );
            assert_eq!(client.delete(&colleague).send().await.status, 403);
            assert_eq!(
                client.get(&format!("{}/audit-events", team.path())).send().await.status,
                403
            );
        }

        assert_eq!(
            world
                .as_user(&team.owner)
                .post(&invitations)
                .json(&invite)
                .send()
                .await
                .status,
            201
        );
    }
}
