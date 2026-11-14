//! OWASP API3:2023 Broken Object Property Level Authorization: a client cannot set `status`, `claimantId`,
//! owners or team ids, and responses show only the documented fields.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api3BrokenObjectPropertyLevelAuthorizationTest.java`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeSet;

use ledgerly_parity_tests::{V1, World, expense_json};
use serde_json::{Value, json};
use uuid::Uuid;

/// `body` with one more property, as Java splices `"claimantId":"…"` into its JSON text.
fn with(mut body: Value, name: &str, value: Value) -> Value {
    body.as_object_mut().unwrap().insert(name.to_owned(), value);
    body
}

fn keys(body: &Value) -> BTreeSet<&str> {
    body.as_object().unwrap().keys().map(String::as_str).collect()
}

mod m6 {
    use super::*;

    /// Java: `Api3BrokenObjectPropertyLevelAuthorizationTest.patchWithStatusIsRejectedAndNothingChanges`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn patch_with_status_is_rejected_and_nothing_changes() {
        let world = World::start().await;
        let team = world.team().await;
        let created = world
            .create_expense(&team, &team.member, &expense_json("10.00", "Cafe"))
            .await;
        let path = format!("{}/expenses/{}", team.path(), created.json()["id"].as_str().unwrap());

        let patch = world
            .as_user(&team.member)
            .patch(&path)
            .header("If-Match", created.header("etag").unwrap())
            .json(&json!({"status": "APPROVED"}))
            .send()
            .await;

        assert_eq!(patch.status, 400);
        assert_eq!(
            world.as_user(&team.member).get(&path).send().await.json()["status"],
            "DRAFT"
        );
    }

    /// Java: `Api3BrokenObjectPropertyLevelAuthorizationTest.roleAndIdentityFieldsCannotBeSetThroughTheTeamEndpoints`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn role_and_identity_fields_cannot_be_set_through_the_team_endpoints() {
        let world = World::start().await;
        let team = world.team().await;

        let create = world
            .as_user(&team.member)
            .post(&format!("{V1}/teams"))
            .json(&json!({"name": "X", "owner": team.colleague.id()}))
            .send()
            .await;
        let change_role = world
            .as_user(&team.owner)
            .patch(&format!("{}/members/{}", team.path(), team.colleague.id()))
            .json(&json!({"role": "MEMBER", "teamId": Uuid::new_v4()}))
            .send()
            .await;

        assert_eq!(create.status, 400);
        assert_eq!(change_role.status, 400);
    }

    /// Java: `Api3BrokenObjectPropertyLevelAuthorizationTest.responsesShowOnlyTheDocumentedFields`.
    #[tokio::test]
    #[ignore = "M6"]
    async fn responses_show_only_the_documented_fields() {
        let world = World::start().await;
        let team = world.team().await;
        let id = world.expense(&team, &team.member, "DRAFT").await;

        let expense = world
            .as_user(&team.member)
            .get(&team.expense_path(id))
            .send()
            .await
            .json();
        let me = world.as_user(&team.member).get(&format!("{V1}/me")).send().await.json();

        // Not the version, the audit columns, or anything else from the database row.
        let documented = BTreeSet::from([
            "id",
            "teamId",
            "claimantId",
            "money",
            "category",
            "incurredOn",
            "merchant",
            "note",
            "status",
            "rejectionReasons",
        ]);
        assert!(keys(&expense).is_subset(&documented), "{expense}");
        for hidden in ["version", "createdAt", "updatedAt"] {
            assert!(!keys(&expense).contains(hidden), "{expense}");
        }
        assert_eq!(
            keys(&me),
            BTreeSet::from(["id", "email", "verified", "teams", "invitations"])
        );
    }
}

mod m8 {
    use super::*;

    /// Java: `Api3BrokenObjectPropertyLevelAuthorizationTest.claimantIdCannotBeSetOnCreateEditOrBatch`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn claimant_id_cannot_be_set_on_create_edit_or_batch() {
        let world = World::start().await;
        let team = world.team().await;
        let member = world.as_user(&team.member);
        let sneaky = json!(team.colleague.id());

        let create = world
            .create_expense(
                &team,
                &team.member,
                &with(expense_json("10.00", "x"), "claimantId", sneaky.clone()),
            )
            .await;
        assert_eq!(create.status, 400);

        let ok = world
            .create_expense(&team, &team.member, &expense_json("10.00", "ok"))
            .await;
        let path = format!("{}/expenses/{}", team.path(), ok.json()["id"].as_str().unwrap());
        let edit = member
            .patch(&path)
            .header("If-Match", ok.header("etag").unwrap())
            .json(&json!({"claimantId": sneaky}))
            .send()
            .await;
        assert_eq!(edit.status, 400);

        let batch = member
            .post(&format!("{}/expenses:batch", team.path()))
            .json(&json!({"items": [with(expense_json("10.00", "x"), "claimantId", sneaky)]}))
            .send()
            .await;
        assert_eq!(batch.status, 400);

        assert_eq!(
            member.get(&path).send().await.json()["claimantId"],
            team.member.id().to_string()
        );
    }
}
