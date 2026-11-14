//! What happens to receipts while the malware scanner gives no answer.
//!
//! Java flips `FakeMalwareScanner.unavailable`. Here the file runs in the `scanner-down` group, in which the
//! ClamAV container is stopped (ADR 0011).

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::World;
use ledgerly_parity_tests::fixtures::PDF;

mod m7 {
    use super::*;

    /// Java: `Api10UnsafeConsumptionOfApisTest.aFileIsNeverAcceptedUntilTheScannerHasClearlySaidSo`, and the
    /// same statement in `ReceiptsApiTest.whileTheScannerIsDownTheReceiptStaysPendingAndCannotBeDownloaded`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn a_file_is_never_accepted_until_the_scanner_has_clearly_said_so() {
        let world = World::start().await;
        let team = world.team().await;
        let expense = world.expense(&team, &team.member, "DRAFT").await;
        let path = format!("{}/receipts", team.expense_path(expense));
        let member = world.as_user(&team.member);

        let uploaded = member
            .post(&path)
            .file("file", "r.pdf", "application/pdf", PDF)
            .send()
            .await
            .json();
        let id = uploaded["id"].as_str().unwrap();

        assert_eq!(member.get(&path).send().await.json()[0]["status"], "PENDING_SCAN");
        assert_eq!(member.get(&format!("{path}/{id}/download")).send().await.status, 404);
    }

    /// Java: `ReceiptsApiTest.aReceiptThatIsStillPendingCannotBeDownloaded`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn a_receipt_that_is_still_pending_cannot_be_downloaded() {
        let world = World::start().await;
        let team = world.team().await;
        let expense = world.expense(&team, &team.member, "DRAFT").await;
        let path = format!("{}/receipts", team.expense_path(expense));
        let member = world.as_user(&team.member);
        let id = member
            .post(&path)
            .file("file", "lunch.pdf", "application/pdf", PDF)
            .send()
            .await
            .json()["id"]
            .as_str()
            .unwrap()
            .to_owned();

        assert_eq!(member.get(&format!("{path}/{id}/download")).send().await.status, 404);
        assert_eq!(member.get(&path).send().await.json()[0]["status"], "PENDING_SCAN");
    }

    /// Java: `ReceiptsApiTest.submitNeedsAnAcceptedReceiptAndNoPendingOne`, up to the point where Java calls
    /// the scan again in-process (see `NOT_PARITY.md`).
    #[tokio::test]
    #[ignore = "M7"]
    async fn submit_needs_an_accepted_receipt_and_no_pending_one() {
        let world = World::start().await;
        let team = world.team().await;
        let expense = world.expense(&team, &team.member, "DRAFT").await;
        let member = world.as_user(&team.member);
        let submit = format!("{}:submit", team.expense_path(expense));

        // No receipt at all.
        assert_eq!(member.post(&submit).send().await.status, 422);
        // Uploaded but the scanner is down: still pending, still refused.
        member
            .post(&format!("{}/receipts", team.expense_path(expense)))
            .file("file", "lunch.pdf", "application/pdf", PDF)
            .send()
            .await;
        let pending = member.post(&submit).send().await;
        assert_eq!(pending.status, 422);
        assert_eq!(pending.problem_code().as_deref(), Some("EXPENSE_RULE_VIOLATED"));
    }
}
