//! No secret and no personal data reaches the logs, whatever the application is asked to do.
//!
//! Java: `src/test/java/com/ledgerly/owasp/LogHygieneTest.java`, which captures the JVM's stdout. Here the log
//! is the stdout of the app's container, which `just parity` names in `PARITY_APP_CONTAINER`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::time::Duration;

use ledgerly_parity_tests::{World, eventually, expense_json};
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "Hunter2-Is-A-Terrible-Passw0rd";
const WRONG_PASSWORD: &str = "Another-Wrong-Passw0rd-1";

mod m10 {
    use super::*;

    /// Java: `LogHygieneTest.noSecretOrPersonalDataReachesTheLogs`.
    #[tokio::test]
    #[ignore = "M10"]
    async fn no_secret_or_personal_data_reaches_the_logs() {
        let world = World::start().await;
        let email = format!("log-hygiene-{}@private-domain.example", Uuid::new_v4());
        let invitee_email = format!("invitee-{}@private-domain.example", Uuid::new_v4());
        let receipt_marker = format!("RECEIPT-CONTENT-MARKER-{}", Uuid::new_v4());
        let receipt_name = format!("secret-receipt-name-{}.pdf", Uuid::new_v4());
        let merchant_marker = format!("MerchantMarker{}", &Uuid::new_v4().to_string()[..8]);
        let note_marker = format!("NoteMarker{}", &Uuid::new_v4().to_string()[..8]);

        // --- accounts: register, verify, log in (right and wrong), refresh (and replay), log out
        assert_eq!(world.register(&email, PASSWORD).await.status, 202);
        let user = ledgerly_parity_tests::fixtures::User {
            id: world.user_id_of(&email).await,
            email: email.clone(),
        };
        // Java reads the token from its mailbox; here it is one the test wrote itself (ADR 0011).
        let verification_token = world.fixtures.verification_token(&user).await;
        assert_eq!(world.verify(&verification_token).await.status, 204);
        world.login(&email, WRONG_PASSWORD).await;
        let login = world.login(&email, PASSWORD).await;
        assert_eq!(login.status, 200);
        let access_token = login.json()["accessToken"].as_str().unwrap().to_owned();
        let refresh_cookie = login.cookie("refresh_token").unwrap();
        let refreshed = world.refresh(&refresh_cookie).await;
        let refreshed_cookie = refreshed.cookie("refresh_token").unwrap();
        let refreshed_access = refreshed.json()["accessToken"].as_str().unwrap().to_owned();
        world.refresh(&refresh_cookie).await; // replay: logs a warning
        world.logout(&refreshed_cookie).await;

        // --- work as a team owner: invitation, expense, receipt upload, batch, import, export
        let team = world.team().await;
        let owner = world.as_user(&team.owner);
        owner
            .post(&format!("{}/invitations", team.path()))
            .json(&json!({"email": invitee_email, "role": "MEMBER"}))
            .send()
            .await;
        let created = world
            .create_expense(&team, &team.owner, &expense_json("12.50", &merchant_marker))
            .await;
        let expense_path = format!("{}/expenses/{}", team.path(), created.json()["id"].as_str().unwrap());
        owner
            .post(&format!("{expense_path}/receipts"))
            .file(
                "file",
                &receipt_name,
                "application/pdf",
                format!("%PDF-1.4\n{receipt_marker}\n%%EOF\n"),
            )
            .send()
            .await;
        owner
            .post(&format!("{}/expenses:batch", team.path()))
            .json(&json!({"items": [expense_json("1.00", &format!("{merchant_marker}b"))]}))
            .send()
            .await;
        let csv = format!(
            "date,merchant,amount,currency,category,note\n\
             2026-01-02,{merchant_marker}i,5.00,EUR,MEALS,{note_marker}\n\
             2026-01-03,Bad row {merchant_marker},abc,EUR,MEALS,\n"
        );
        let imported = owner
            .post(&format!("{}/imports", team.path()))
            .file("file", &format!("import-{receipt_marker}.csv"), "text/csv", csv)
            .send()
            .await;
        let import = format!("{}/imports/{}", team.path(), imported.json()["id"].as_str().unwrap());
        eventually("the import to complete", Duration::from_secs(30), || async {
            (owner.get(&import).send().await.json()["status"] == "COMPLETED").then_some(())
        })
        .await;
        owner.get(&format!("{}/expenses.csv", team.path())).send().await;
        // An authorization failure, which is logged too.
        world
            .as_user(&team.colleague)
            .get(&team.expense_path(Uuid::new_v4()))
            .send()
            .await;
        // Let the asynchronous listeners (scan, audit) write their log lines as well.
        eventually("five audit events of the team", Duration::from_secs(20), || async {
            let count: i64 = sqlx::query_scalar("select count(*) from audit_events where team_id = $1")
                .bind(team.id)
                .fetch_one(world.fixtures.pool())
                .await
                .unwrap();
            (count >= 5).then_some(())
        })
        .await;

        let logs = world.target.app_logs();
        assert!(!logs.trim().is_empty(), "the logs of the flows above are not empty");
        let secrets = [
            email.as_str(),
            &invitee_email,
            "@private-domain.example",
            PASSWORD,
            WRONG_PASSWORD,
            &verification_token,
            &access_token,
            &refreshed_access,
            &refresh_cookie,
            &refreshed_cookie,
            &team.owner.token,
            "Bearer ",
            &receipt_marker,
            &receipt_name,
            &merchant_marker,
            &note_marker,
            "password_hash",
            "argon2id",
        ];
        for secret in secrets {
            assert!(!logs.contains(secret), "the logs must not contain '{secret}'");
        }
        // What is allowed: ids. The user id and team id are on the request lines.
        assert!(logs.contains(&team.id.to_string()), "the logs name the team by id");
    }
}
