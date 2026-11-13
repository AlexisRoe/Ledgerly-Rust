//! Not ported from Java: checks that the harness itself can do what the ported tests rely on. If a migration
//! changes a table, this fails here, with a clear message, and not in fifty tests at once.

use ledgerly_parity_tests::Target;
use ledgerly_parity_tests::encoding::sha256_hex;
use ledgerly_parity_tests::fixtures::Expense;
use sqlx::Row;

mod m1 {
    use super::*;

    #[tokio::test]
    async fn fixtures_can_write_users_teams_expenses_and_verifications() {
        let target = Target::from_env();
        let fixtures = target.fixtures().await;

        let team = fixtures.team_with_roles().await;
        let unverified = fixtures.unverified_user().await;
        let token = fixtures.verification_token(&unverified).await;
        let draft = fixtures.expense(team.id, &team.member, "DRAFT").await;
        let rejected = fixtures
            .insert_expense(&Expense {
                note: Some("with a note".to_owned()),
                ..Expense::new(team.id, team.colleague.id, "REJECTED")
            })
            .await;
        fixtures.rejection(rejected, &team.approver, "first").await;
        fixtures.rejection(rejected, &team.owner, "second").await;

        let roles: Vec<String> = sqlx::query("select role from memberships where team_id = $1 order by role")
            .bind(team.id)
            .fetch_all(fixtures.pool())
            .await
            .unwrap()
            .iter()
            .map(|row| row.get("role"))
            .collect();
        assert_eq!(roles, ["APPROVER", "MEMBER", "MEMBER", "OWNER"]);

        let user = sqlx::query("select verified, password_hash from users where id = $1")
            .bind(unverified.id)
            .fetch_one(fixtures.pool())
            .await
            .unwrap();
        assert!(!user.get::<bool, _>("verified"));
        assert!(
            user.get::<String, _>("password_hash")
                .starts_with("$argon2id$v=19$m=16384,t=2,p=1$")
        );

        let stored: String = sqlx::query("select token_hash from email_verifications where user_id = $1")
            .bind(unverified.id)
            .fetch_one(fixtures.pool())
            .await
            .unwrap()
            .get("token_hash");
        assert_eq!(token.len(), 43, "32 bytes as unpadded Base64url");
        assert_eq!(stored, sha256_hex(token.as_bytes()));

        let status: String = sqlx::query("select status from expenses where id = $1 and team_id = $2")
            .bind(draft)
            .bind(team.id)
            .fetch_one(fixtures.pool())
            .await
            .unwrap()
            .get("status");
        assert_eq!(status, "DRAFT");

        let reasons: Vec<String> =
            sqlx::query("select reason from expense_rejections where expense_id = $1 order by position")
                .bind(rejected)
                .fetch_all(fixtures.pool())
                .await
                .unwrap()
                .iter()
                .map(|row| row.get("reason"))
                .collect();
        assert_eq!(reasons, ["first", "second"]);

        fixtures.remove_member(team.id, &team.colleague).await;
        fixtures.member(team.id, &team.member, "APPROVER").await;
        let approvers: i64 =
            sqlx::query("select count(*) as n from memberships where team_id = $1 and role = 'APPROVER'")
                .bind(team.id)
                .fetch_one(fixtures.pool())
                .await
                .unwrap()
                .get("n");
        assert_eq!(approvers, 2);
    }

    #[tokio::test]
    async fn an_accepted_receipt_has_a_row_and_an_object_in_the_store() {
        let target = Target::from_env();
        let fixtures = target.fixtures().await;
        let team = fixtures.team_with_roles().await;
        let expense = fixtures.expense(team.id, &team.member, "DRAFT").await;

        let receipt = fixtures.accepted_receipt(team.id, expense, &team.member).await;

        let row = sqlx::query("select storage_key, sha256, size_bytes, scan_status from receipts where id = $1")
            .bind(receipt)
            .fetch_one(fixtures.pool())
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("scan_status"), "ACCEPTED");
        let key: String = row.get("storage_key");
        assert!(key.starts_with("receipts/"), "{key}");

        let object = reqwest::get(format!("{}/{}/{key}", target.s3_endpoint, target.s3_bucket))
            .await
            .unwrap();
        assert_eq!(object.status(), 200);
        let bytes = object.bytes().await.unwrap();
        assert_eq!(sha256_hex(&bytes), row.get::<String, _>("sha256"));
        assert_eq!(i64::try_from(bytes.len()).unwrap(), row.get::<i64, _>("size_bytes"));
    }
}
