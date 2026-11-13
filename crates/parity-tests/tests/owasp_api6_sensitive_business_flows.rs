//! OWASP API6:2023 Unrestricted Access to Sensitive Business Flows: no self-dealing, invitations never reveal
//! which emails are registered, imports are capped per day.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api6UnrestrictedAccessToSensitiveBusinessFlowsTest.java`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::World;
use serde_json::json;
use uuid::Uuid;

mod m6 {
    use super::*;

    /// Java: `Api6UnrestrictedAccessToSensitiveBusinessFlowsTest.anApproverCannotApproveOrRejectTheirOwnExpense`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn an_approver_cannot_approve_or_reject_their_own_expense() {
        let world = World::start().await;
        let team = world.team().await;
        let mine = world.expense(&team, &team.approver, "SUBMITTED").await;
        let approver = world.as_user(&team.approver);

        let approve = approver
            .post(&format!("{}:approve", team.expense_path(mine)))
            .send()
            .await;
        let reject = approver
            .post(&format!("{}:reject", team.expense_path(mine)))
            .json(&json!({"reason": "x"}))
            .send()
            .await;

        assert_eq!(approve.status, 409);
        assert_eq!(approve.problem_code().as_deref(), Some("EXPENSE_SELF_DEALING"));
        assert_eq!(reject.status, 409);
    }

    /// Java: `Api6UnrestrictedAccessToSensitiveBusinessFlowsTest.anOwnerCannotReimburseTheirOwnExpense`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn an_owner_cannot_reimburse_their_own_expense() {
        let world = World::start().await;
        let team = world.team().await;
        let mine = world.expense(&team, &team.owner, "APPROVED").await;

        let response = world
            .as_user(&team.owner)
            .post(&format!("{}:reimburse", team.expense_path(mine)))
            .send()
            .await;

        assert_eq!(response.status, 409);
        assert_eq!(response.problem_code().as_deref(), Some("EXPENSE_SELF_DEALING"));
    }

    /// Java: `Api6UnrestrictedAccessToSensitiveBusinessFlowsTest.somebodyElseCanStillDecideSoTheRuleIsNotJustABrokenWorkflow`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn somebody_else_can_still_decide_so_the_rule_is_not_just_a_broken_workflow() {
        let world = World::start().await;
        let team = world.team().await;
        let theirs = world.expense(&team, &team.approver, "SUBMITTED").await;

        let response = world
            .as_user(&team.owner)
            .post(&format!("{}:approve", team.expense_path(theirs)))
            .send()
            .await;

        assert_eq!(response.status, 200);
    }

    /// Java: `Api6UnrestrictedAccessToSensitiveBusinessFlowsTest.invitingARegisteredAndAnUnknownEmailLooksTheSame`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn inviting_a_registered_and_an_unknown_email_looks_the_same() {
        let world = World::start().await;
        let team = world.team().await;
        let registered = world.fixtures.user().await;
        let invite = |email: String| {
            world
                .as_user(&team.owner)
                .post(&format!("{}/invitations", team.path()))
                .json(&json!({"email": email, "role": "MEMBER"}))
                .send()
        };

        let known = invite(registered.email).await;
        let unknown = invite(format!("nobody-{}@example.com", Uuid::new_v4())).await;

        assert_eq!(known.status, unknown.status);
        assert_eq!(known.property_names(), unknown.property_names());
        assert_eq!(known.json()["status"], unknown.json()["status"]);
        assert_eq!(known.json()["role"], unknown.json()["role"]);
    }
}

mod m8 {
    use super::*;

    /// Java: `Api6UnrestrictedAccessToSensitiveBusinessFlowsTest.importsAreCappedAtFivePerDay`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn imports_are_capped_at_five_per_day() {
        let world = World::start().await;
        let team = world.team().await;
        let csv = "date,merchant,amount,currency,category\n2026-01-01,Cafe,1.00,EUR,MEALS\n";
        let upload = || {
            world
                .as_user(&team.colleague)
                .post(&format!("{}/imports", team.path()))
                .file("file", "e.csv", "text/csv", csv)
                .send()
        };

        for _ in 0..5 {
            assert_eq!(upload().await.status, 202);
        }

        assert_eq!(upload().await.status, 429);
    }
}
