//! Creating, reading, editing, deleting and moving one expense through the workflow.
//!
//! Java: `src/test/java/com/ledgerly/expenses/internal/web/ExpensesApiTest.java`. Java mints tokens for random
//! user ids and seeds their memberships; here the users of [`World::team`] log in. Its `claimant` is the team's
//! MEMBER, and "another member" is the colleague.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use chrono::{Days, Utc};
use ledgerly_parity_tests::http::Response;
use ledgerly_parity_tests::{SignedInTeam, V1, World, today};
use serde_json::{Value, json};
use uuid::Uuid;

/// Java: `ExpensesApiTest.body()`.
fn body(amount: &str, currency: &str, date: &str, merchant: &str) -> Value {
    json!({
        "money": {"amount": amount, "currency": currency},
        "category": "MEALS",
        "incurredOn": date,
        "merchant": merchant,
    })
}

fn valid_body() -> Value {
    body("12.50", "EUR", &today(), "Cafe")
}

fn expenses(team: &SignedInTeam) -> String {
    format!("{}/expenses", team.path())
}

fn id(response: &Response) -> String {
    response.json()["id"].as_str().unwrap().to_owned()
}

fn etag(response: &Response) -> String {
    response.header("etag").expect("the response has an ETag").to_owned()
}

/// Java: `createDraft()`: a draft of the MEMBER.
async fn create_draft(world: &World, team: &SignedInTeam) -> Response {
    let response = world.create_expense(team, &team.member, &valid_body()).await;
    assert_eq!(response.status, 201);
    response
}

/// Java: `submitted()`: a draft with an accepted receipt, submitted by the MEMBER.
async fn submitted(world: &World, team: &SignedInTeam) -> Response {
    let draft = create_draft(world, team).await;
    world
        .fixtures
        .accepted_receipt(team.id, id(&draft).parse().unwrap(), &team.member.user)
        .await;
    let response = world
        .as_user(&team.member)
        .post(&format!("{}/{}:submit", expenses(team), id(&draft)))
        .send()
        .await;
    assert_eq!(response.status, 200);
    response
}

mod m3 {
    use super::*;

    /// Java: `ExpensesApiTest.unknownPathIsAProblemToo`. Java sends a token; the 404 with `REQUEST_REJECTED`
    /// for a route that does not exist is what M3 builds, and the token check comes in M5.
    #[tokio::test]
    #[ignore = "M3"]
    async fn unknown_path_is_a_problem_too() {
        let world = World::start().await;

        let response = world.anonymous().get(&format!("{V1}/nope")).send_unchecked().await;

        assert_eq!(response.status, 404);
        assert_eq!(response.problem_code().as_deref(), Some("REQUEST_REJECTED"));
    }
}

mod m6 {
    use super::*;

    // --- happy paths ---

    /// Java: `ExpensesApiTest.createReturns201WithLocationAndEtag`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn create_returns_201_with_location_and_etag() {
        let world = World::start().await;
        let team = world.team().await;

        let response = create_draft(&world, &team).await;

        let location = response.header("location").unwrap();
        assert!(
            location.ends_with(&format!("{}/{}", expenses(&team), id(&response))),
            "{location}"
        );
        assert_eq!(etag(&response), "\"0\"");
        let body = response.json();
        assert_eq!(body["status"], "DRAFT");
        assert_eq!(body["money"]["amount"], "12.50");
        assert_eq!(body["claimantId"], team.member.id().to_string());
    }

    /// Java: `ExpensesApiTest.getReturnsTheExpenseWithEtag`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn get_returns_the_expense_with_etag() {
        let world = World::start().await;
        let team = world.team().await;
        let created = create_draft(&world, &team).await;

        let response = world
            .as_user(&team.member)
            .get(&format!("{}/{}", expenses(&team), id(&created)))
            .send()
            .await;

        assert_eq!(response.status, 200);
        assert_eq!(etag(&response), etag(&created));
    }

    /// Java: `ExpensesApiTest.patchWithIfMatchUpdatesAndBumpsEtag`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn patch_with_if_match_updates_and_bumps_etag() {
        let world = World::start().await;
        let team = world.team().await;
        let created = create_draft(&world, &team).await;

        let response = world
            .as_user(&team.member)
            .patch(&format!("{}/{}", expenses(&team), id(&created)))
            .header("If-Match", &etag(&created))
            .json(&json!({"merchant": "Bakery", "note": "rolls"}))
            .send()
            .await;

        assert_eq!(response.status, 200);
        assert_eq!(response.json()["merchant"], "Bakery");
        assert_eq!(response.json()["note"], "rolls");
        assert_eq!(etag(&response), "\"1\"");
    }

    /// Java: `ExpensesApiTest.deleteDraftReturns204ThenGone`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn delete_draft_returns_204_then_gone() {
        let world = World::start().await;
        let team = world.team().await;
        let created = create_draft(&world, &team).await;
        let path = format!("{}/{}", expenses(&team), id(&created));
        let member = world.as_user(&team.member);

        let deleted = member.delete(&path).header("If-Match", &etag(&created)).send().await;

        assert_eq!(deleted.status, 204);
        assert_eq!(member.get(&path).send().await.status, 404);
    }

    // --- abuse tests ---

    /// Java: `ExpensesApiTest.patchCannotSetStatusOrClaimant_API3`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn patch_cannot_set_status_or_claimant() {
        let world = World::start().await;
        let team = world.team().await;
        let created = create_draft(&world, &team).await;
        let path = format!("{}/{}", expenses(&team), id(&created));
        let member = world.as_user(&team.member);

        let status = member
            .patch(&path)
            .header("If-Match", &etag(&created))
            .json(&json!({"status": "APPROVED"}))
            .send()
            .await;
        let claimant = member
            .patch(&path)
            .header("If-Match", &etag(&created))
            .json(&json!({"claimantId": Uuid::new_v4()}))
            .send()
            .await;

        assert_eq!(status.status, 400);
        assert_eq!(status.problem_code().as_deref(), Some("MALFORMED_REQUEST"));
        assert_eq!(claimant.status, 400);
    }

    /// Java: `ExpensesApiTest.approvingYourOwnExpenseGets409`. Java submits without a receipt and ignores the
    /// answer, so the expense it approves is still a draft; the role and self-dealing checks come before the
    /// state check.
    #[tokio::test]
    #[ignore = "M6"]
    async fn approving_your_own_expense_gets_409() {
        let world = World::start().await;
        let team = world.team().await;
        let approver = world.as_user(&team.approver);
        let draft = world.create_expense(&team, &team.approver, &valid_body()).await;
        let path = format!("{}/{}", expenses(&team), id(&draft));
        approver.post(&format!("{path}:submit")).send().await;

        let response = approver.post(&format!("{path}:approve")).send().await;

        assert_eq!(response.status, 409);
        assert_eq!(response.problem_code().as_deref(), Some("EXPENSE_SELF_DEALING"));
    }

    /// Java: `ExpensesApiTest.illegalTransitionGets409`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn illegal_transition_gets_409() {
        let world = World::start().await;
        let team = world.team().await;
        let draft = create_draft(&world, &team).await;

        let response = world
            .as_user(&team.approver)
            .post(&format!("{}/{}:approve", expenses(&team), id(&draft)))
            .send()
            .await;

        assert_eq!(response.status, 409);
        assert_eq!(response.problem_code().as_deref(), Some("EXPENSE_INVALID_TRANSITION"));
        assert_eq!(
            response.json()["type"],
            "https://ledgerly.dev/problems/invalid-transition"
        );
    }

    /// Java: `ExpensesApiTest.tooManyDecimalsGets400WithFieldError`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn too_many_decimals_gets_400_with_field_error() {
        let world = World::start().await;
        let team = world.team().await;

        let response = world
            .create_expense(&team, &team.member, &body("12.345", "EUR", &today(), "x"))
            .await;

        assert_eq!(response.status, 400);
        assert_eq!(response.json()["errors"][0]["field"], "money");
    }

    /// Java: `ExpensesApiTest.futureDateGets400`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn future_date_gets_400() {
        let world = World::start().await;
        let team = world.team().await;
        let in_two_days = Utc::now()
            .date_naive()
            .checked_add_days(Days::new(2))
            .unwrap()
            .to_string();

        let response = world
            .create_expense(&team, &team.member, &body("1.00", "EUR", &in_two_days, "x"))
            .await;

        assert_eq!(response.status, 400);
        assert_eq!(response.json()["errors"][0]["field"], "incurredOn");
    }

    /// Java: `ExpensesApiTest.beanValidationErrorsAreListedPerField`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn validation_errors_are_listed_per_field() {
        let world = World::start().await;
        let team = world.team().await;

        let response = world
            .create_expense(&team, &team.member, &json!({"category": "MEALS", "merchant": ""}))
            .await;

        assert_eq!(response.status, 400);
        assert_eq!(response.problem_code().as_deref(), Some("VALIDATION_FAILED"));
        assert!(response.json()["errors"].as_array().unwrap().len() >= 2);
    }

    /// Java: `ExpensesApiTest.malformedJsonGets400Problem`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn malformed_json_gets_400_problem() {
        let world = World::start().await;
        let team = world.team().await;

        let response = world
            .as_user(&team.member)
            .post(&expenses(&team))
            .body("application/json", "{not json")
            .send()
            .await;

        assert_eq!(response.status, 400);
        assert_eq!(response.problem_code().as_deref(), Some("MALFORMED_REQUEST"));
    }

    /// Java: `ExpensesApiTest.missingIfMatchGets428AndStaleGets412`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn missing_if_match_gets_428_and_stale_gets_412() {
        let world = World::start().await;
        let team = world.team().await;
        let created = create_draft(&world, &team).await;
        let path = format!("{}/{}", expenses(&team), id(&created));
        let member = world.as_user(&team.member);
        let edit = json!({"merchant": "A"});

        assert_eq!(member.patch(&path).json(&edit).send().await.status, 428);
        assert_eq!(member.delete(&path).send().await.status, 428);
        let stale = member
            .patch(&path)
            .header("If-Match", "\"99\"")
            .json(&edit)
            .send()
            .await;
        assert_eq!(stale.status, 412);
        assert_eq!(stale.problem_code().as_deref(), Some("PRECONDITION_FAILED"));
        assert_eq!(
            member.delete(&path).header("If-Match", "garbage").send().await.status,
            412
        );
    }

    /// Java: `ExpensesApiTest.secondPatchWithOldEtagGets412`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn second_patch_with_old_etag_gets_412() {
        let world = World::start().await;
        let team = world.team().await;
        let created = create_draft(&world, &team).await;
        let path = format!("{}/{}", expenses(&team), id(&created));
        let member = world.as_user(&team.member);

        member
            .patch(&path)
            .header("If-Match", &etag(&created))
            .json(&json!({"merchant": "A"}))
            .send()
            .await;
        let again = member
            .patch(&path)
            .header("If-Match", &etag(&created))
            .json(&json!({"merchant": "B"}))
            .send()
            .await;

        assert_eq!(again.status, 412);
    }

    /// Java: `ExpensesApiTest.anotherMemberCannotSeeOrTouchMyExpense_API1`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn another_member_cannot_see_or_touch_my_expense() {
        let world = World::start().await;
        let team = world.team().await;
        let draft = create_draft(&world, &team).await;
        let path = format!("{}/{}", expenses(&team), id(&draft));
        let colleague = world.as_user(&team.colleague);

        assert_eq!(colleague.get(&path).send().await.status, 404);
        assert_eq!(
            colleague
                .patch(&path)
                .header("If-Match", &etag(&draft))
                .json(&json!({"merchant": "x"}))
                .send()
                .await
                .status,
            404
        );
        assert_eq!(colleague.post(&format!("{path}:submit")).send().await.status, 404);
    }

    /// Java: `ExpensesApiTest.nonTeammateGets404EvenWithAValidId_API1`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn non_teammate_gets_404_even_with_a_valid_id() {
        let world = World::start().await;
        let team = world.team().await;
        let draft = create_draft(&world, &team).await;
        let outsider = world.as_user(&world.user().await);

        assert_eq!(
            outsider
                .get(&format!("{}/{}", expenses(&team), id(&draft)))
                .send()
                .await
                .status,
            404
        );
        assert_eq!(
            outsider.post(&expenses(&team)).json(&valid_body()).send().await.status,
            404
        );
    }

    /// Java: `ExpensesApiTest.expenseOfTeamAIsNotFoundThroughTeamB_API1`. Java makes the caller an OWNER of the
    /// random team in the path; here it is the owner of a second team.
    #[tokio::test]
    #[ignore = "M6"]
    async fn expense_of_team_a_is_not_found_through_team_b() {
        let world = World::start().await;
        let (a, b) = (world.team().await, world.team().await);
        let draft = create_draft(&world, &a).await;

        let response = world
            .as_user(&b.owner)
            .get(&format!("{}/{}", expenses(&b), id(&draft)))
            .send()
            .await;

        assert_eq!(response.status, 404);
    }

    /// Java: `ExpensesApiTest.approverCanSeeAnotherMembersExpense`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn approver_can_see_another_members_expense() {
        let world = World::start().await;
        let team = world.team().await;
        let draft = create_draft(&world, &team).await;

        let response = world
            .as_user(&team.approver)
            .get(&format!("{}/{}", expenses(&team), id(&draft)))
            .send()
            .await;

        assert_eq!(response.status, 200);
    }

    /// Java: `ExpensesApiTest.missingTokenGets401`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn missing_token_gets_401() {
        let world = World::start().await;
        let team = world.team().await;

        let response = world
            .anonymous()
            .get(&format!("{}/{}", expenses(&team), Uuid::new_v4()))
            .send()
            .await;

        assert_eq!(response.status, 401);
        assert_eq!(response.problem_code().as_deref(), Some("UNAUTHENTICATED"));
    }

    // --- idempotency ---

    async fn create_with_key(
        world: &World,
        team: &SignedInTeam,
        user: &ledgerly_parity_tests::Session,
        body: &Value,
        key: &str,
    ) -> Response {
        world
            .as_user(user)
            .post(&expenses(team))
            .header("Idempotency-Key", key)
            .json(body)
            .send()
            .await
    }

    /// Java: `ExpensesApiTest.sameKeySameBodyReplaysTheOriginal`. The key is fresh per run: the database is
    /// shared and never cleaned, and Java's literal key would replay an earlier run.
    #[tokio::test]
    #[ignore = "M6"]
    async fn same_key_same_body_replays_the_original() {
        let world = World::start().await;
        let team = world.team().await;
        let key = format!("key-{}", Uuid::new_v4().simple());

        let first = create_with_key(&world, &team, &team.member, &valid_body(), &key).await;
        let second = create_with_key(&world, &team, &team.member, &valid_body(), &key).await;

        assert_eq!((first.status, second.status), (201, 201));
        assert_eq!(id(&second), id(&first));
    }

    /// Java: `ExpensesApiTest.sameKeyDifferentBodyGets422`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn same_key_different_body_gets_422() {
        let world = World::start().await;
        let team = world.team().await;
        let key = format!("key-{}", Uuid::new_v4().simple());

        create_with_key(&world, &team, &team.member, &valid_body(), &key).await;
        let second = create_with_key(
            &world,
            &team,
            &team.member,
            &body("99.00", "EUR", &today(), "Other"),
            &key,
        )
        .await;

        assert_eq!(second.status, 422);
        assert_eq!(second.problem_code().as_deref(), Some("IDEMPOTENCY_KEY_REUSED"));
    }

    /// Java: `ExpensesApiTest.sameKeyFromAnotherUserIsIndependent`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn same_key_from_another_user_is_independent() {
        let world = World::start().await;
        let team = world.team().await;
        let key = format!("shared-key-{}", Uuid::new_v4().simple());

        let a = create_with_key(&world, &team, &team.member, &valid_body(), &key).await;
        let b = create_with_key(&world, &team, &team.colleague, &valid_body(), &key).await;

        assert_eq!(b.status, 201);
        assert_ne!(id(&b), id(&a));
    }

    /// Java: `ExpensesApiTest.withoutKeyEveryPostCreatesANewExpense`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn without_key_every_post_creates_a_new_expense() {
        let world = World::start().await;
        let team = world.team().await;

        assert_ne!(
            id(&create_draft(&world, &team).await),
            id(&create_draft(&world, &team).await)
        );
    }
}

mod m7 {
    use super::*;

    /// Java: `ExpensesApiTest.fullWorkflowDraftToReimbursed`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn full_workflow_draft_to_reimbursed() {
        let world = World::start().await;
        let team = world.team().await;
        let base = format!("{}/{}", expenses(&team), id(&submitted(&world, &team).await));

        let approved = world
            .as_user(&team.approver)
            .post(&format!("{base}:approve"))
            .send()
            .await;
        let reimbursed = world
            .as_user(&team.owner)
            .post(&format!("{base}:reimburse"))
            .send()
            .await;

        assert_eq!(approved.json()["status"], "APPROVED");
        assert_eq!(reimbursed.json()["status"], "REIMBURSED");
    }

    /// Java: `ExpensesApiTest.rejectReopenWithdrawCycle`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn reject_reopen_withdraw_cycle() {
        let world = World::start().await;
        let team = world.team().await;
        let base = format!("{}/{}", expenses(&team), id(&submitted(&world, &team).await));
        let member = world.as_user(&team.member);

        let rejected = world
            .as_user(&team.approver)
            .post(&format!("{base}:reject"))
            .json(&json!({"reason": "blurry"}))
            .send()
            .await
            .json();
        assert_eq!(rejected["status"], "REJECTED");
        assert_eq!(rejected["rejectionReasons"][0], "blurry");
        assert_eq!(
            member.post(&format!("{base}:reopen")).send().await.json()["status"],
            "DRAFT"
        );
        member.post(&format!("{base}:submit")).send().await;
        assert_eq!(
            member.post(&format!("{base}:withdraw")).send().await.json()["status"],
            "DRAFT"
        );
    }

    /// Java: `ExpensesApiTest.memberCallingApproveGets403_API5`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn member_calling_approve_gets_403() {
        let world = World::start().await;
        let team = world.team().await;
        let submitted = submitted(&world, &team).await;

        let response = world
            .as_user(&team.member)
            .post(&format!("{}/{}:approve", expenses(&team), id(&submitted)))
            .send()
            .await;

        assert_eq!(response.status, 403);
        assert_eq!(response.problem_code().as_deref(), Some("FORBIDDEN"));
    }

    /// Java: `ExpensesApiTest.editingOrDeletingANonDraftGets409`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn editing_or_deleting_a_non_draft_gets_409() {
        let world = World::start().await;
        let team = world.team().await;
        let submitted = submitted(&world, &team).await;
        let path = format!("{}/{}", expenses(&team), id(&submitted));
        let member = world.as_user(&team.member);

        let edit = member
            .patch(&path)
            .header("If-Match", &etag(&submitted))
            .json(&json!({"merchant": "A"}))
            .send()
            .await;
        let delete = member.delete(&path).header("If-Match", &etag(&submitted)).send().await;

        assert_eq!(edit.status, 409);
        assert_eq!(edit.problem_code().as_deref(), Some("EXPENSE_NOT_DRAFT"));
        assert_eq!(delete.status, 409);
    }

    /// Java: `ExpensesApiTest.submitNeedsAcceptedReceipt_D3`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn submit_needs_an_accepted_receipt() {
        let world = World::start().await;
        let team = world.team().await;
        let draft = create_draft(&world, &team).await;

        let response = world
            .as_user(&team.member)
            .post(&format!("{}/{}:submit", expenses(&team), id(&draft)))
            .send()
            .await;

        assert_eq!(response.status, 422);
        assert_eq!(response.problem_code().as_deref(), Some("EXPENSE_RULE_VIOLATED"));
    }

    /// Java: `ExpensesApiTest.rejectWithoutReasonGets400`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn reject_without_reason_gets_400() {
        let world = World::start().await;
        let team = world.team().await;
        let submitted = submitted(&world, &team).await;

        let response = world
            .as_user(&team.approver)
            .post(&format!("{}/{}:reject", expenses(&team), id(&submitted)))
            .json(&json!({"reason": ""}))
            .send()
            .await;

        assert_eq!(response.status, 400);
    }
}
