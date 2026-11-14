//! OWASP API1:2023 Broken Object Level Authorization: an id from another team, or from a teammate a MEMBER may
//! not see, is answered exactly like an id that does not exist.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api1BrokenObjectLevelAuthorizationTest.java`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::{V1, World};
use uuid::Uuid;

mod m6 {
    use super::*;

    /// Java: `Api1BrokenObjectLevelAuthorizationTest.aMemberGets404ForATeammatesExpense`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn a_member_gets_404_for_a_teammates_expense() {
        let world = World::start().await;
        let team = world.team().await;
        let expense = world.expense(&team, &team.member, "DRAFT").await;
        let path = team.expense_path(expense);

        assert_eq!(world.as_user(&team.colleague).get(&path).send().await.status, 404);
        assert_eq!(world.as_user(&team.member).get(&path).send().await.status, 200);
        assert_eq!(world.as_user(&team.approver).get(&path).send().await.status, 200);
    }

    /// Java: `Api1BrokenObjectLevelAuthorizationTest.theListNeverShowsAnotherTeamsOrAnotherMembersExpenses`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn the_list_never_shows_another_teams_or_another_members_expenses() {
        let world = World::start().await;
        let (a, b) = (world.team().await, world.team().await);
        let mine = world.expense(&a, &a.member, "DRAFT").await;
        world.expense(&a, &a.colleague, "DRAFT").await;
        world.expense(&b, &b.member, "DRAFT").await;

        let list = world
            .as_user(&a.member)
            .get(&format!("{}/expenses?claimantId={}", a.path(), a.colleague.id()))
            .send()
            .await
            .json();

        let items = list["content"].as_array().unwrap();
        assert_eq!(items.len(), 1, "{list}");
        assert_eq!(items[0]["id"], mine.to_string());
    }

    /// Java: `Api1BrokenObjectLevelAuthorizationTest.anOutsiderLearnsNothingAboutWhichTeamsExist`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn an_outsider_learns_nothing_about_which_teams_exist() {
        let world = World::start().await;
        let outsider = world.user().await;
        let team = world.team().await;

        let real = world
            .as_user(&outsider)
            .get(&format!("{}/expenses", team.path()))
            .send()
            .await;
        let imaginary = world
            .as_user(&outsider)
            .get(&format!("{V1}/teams/{}/expenses", Uuid::new_v4()))
            .send()
            .await;

        assert!(real.problem_code().is_some(), "{}", real.text());
        assert_eq!(real.problem_code(), imaginary.problem_code());
    }
}

mod m7 {
    use super::*;

    /// Java: `Api1BrokenObjectLevelAuthorizationTest.aUserOfTeamAGets404ForTeamBsExpenseAndReceipt`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn a_user_of_team_a_gets_404_for_team_bs_expense_and_receipt() {
        let world = World::start().await;
        let (a, b) = (world.team().await, world.team().await);
        let expense_of_b = world.expense(&b, &b.member, "DRAFT").await;
        let receipt_of_b = world
            .fixtures
            .accepted_receipt(b.id, expense_of_b, &b.member.user)
            .await;
        let owner_of_a = world.as_user(&a.owner);

        // Through team B's own path: A's owner is not a teammate there.
        // Through team A's path with B's ids: the query is scoped to team A, so B's rows do not exist there.
        for team in [&b, &a] {
            let expense = team.expense_path(expense_of_b);
            assert_eq!(owner_of_a.get(&expense).send().await.status, 404);
            assert_eq!(
                owner_of_a
                    .get(&format!("{expense}/receipts/{receipt_of_b}/download"))
                    .send()
                    .await
                    .status,
                404
            );
        }
    }

    /// Java: `Api1BrokenObjectLevelAuthorizationTest.receiptsFollowTheirExpenseNotJustTheirOwnId`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn receipts_follow_their_expense_not_just_their_own_id() {
        let world = World::start().await;
        let team = world.team().await;
        let first = world.expense(&team, &team.member, "DRAFT").await;
        let second = world.expense(&team, &team.member, "DRAFT").await;
        let receipt = world.fixtures.accepted_receipt(team.id, first, &team.member.user).await;

        let through_the_sibling = world
            .as_user(&team.member)
            .get(&format!("{}/receipts/{receipt}/download", team.expense_path(second)))
            .send()
            .await;
        let by_the_colleague = world
            .as_user(&team.colleague)
            .get(&format!("{}/receipts/{receipt}/download", team.expense_path(first)))
            .send()
            .await;

        assert_eq!(through_the_sibling.status, 404);
        assert_eq!(by_the_colleague.status, 404);
    }
}

mod m9 {
    use super::*;

    /// Java: `Api1BrokenObjectLevelAuthorizationTest.theAuditTrailAndImportsOfOtherTeamsAreNotReachable`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn the_audit_trail_and_imports_of_other_teams_are_not_reachable() {
        let world = World::start().await;
        let (a, b) = (world.team().await, world.team().await);
        let owner_of_a = world.as_user(&a.owner);

        assert_eq!(
            owner_of_a
                .get(&format!("{}/audit-events", b.path()))
                .send()
                .await
                .status,
            404
        );
        assert_eq!(
            owner_of_a
                .get(&format!("{}/imports/{}", b.path(), Uuid::new_v4()))
                .send()
                .await
                .status,
            404
        );
    }
}
