//! OWASP API10:2023 Unsafe Consumption of APIs: what comes from outside (CSV files, uploads, the scanner's
//! answer) is not trusted, and what leaves in a CSV cannot run as a formula.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api10UnsafeConsumptionOfApisTest.java`.
//!
//! Its test with the scanner switched off is in `receipts_scanner_outage.rs`, which runs with ClamAV stopped.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::time::Duration;

use ledgerly_parity_tests::fixtures::{EXE, infected_pdf};
use ledgerly_parity_tests::{World, eventually};

mod m7 {
    use super::*;

    /// Java: `Api10UnsafeConsumptionOfApisTest.anInfectedFileIsQuarantinedAndNeverServed`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn an_infected_file_is_quarantined_and_never_served() {
        let world = World::start().await;
        let team = world.team().await;
        let expense = world.expense(&team, &team.member, "DRAFT").await;
        let path = format!("{}/receipts", team.expense_path(expense));
        let member = world.as_user(&team.member);

        let uploaded = member
            .post(&path)
            .file("file", "r.pdf", "application/pdf", infected_pdf())
            .send()
            .await
            .json();
        let id = uploaded["id"].as_str().unwrap();
        eventually("the receipt to be quarantined", Duration::from_secs(20), || async {
            (member.get(&path).send().await.json()[0]["status"] == "QUARANTINED").then_some(())
        })
        .await;

        let download = format!("{path}/{id}/download");
        assert_eq!(member.get(&download).send().await.status, 404);
        assert_eq!(world.as_user(&team.approver).get(&download).send().await.status, 404);
    }

    /// Java: `Api10UnsafeConsumptionOfApisTest.theClaimedFileTypeAndNameAreNeverTrusted`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn the_claimed_file_type_and_name_are_never_trusted() {
        let world = World::start().await;
        let team = world.team().await;
        let expense = world.expense(&team, &team.member, "DRAFT").await;
        let path = format!("{}/receipts", team.expense_path(expense));
        let member = world.as_user(&team.member);

        let executable = member
            .post(&path)
            .file("file", "invoice.pdf", "application/pdf", EXE)
            .send()
            .await;
        let html = member
            .post(&path)
            .file("file", "x.png", "image/png", "<html><script>alert(1)</script></html>")
            .send()
            .await;

        assert_eq!(executable.status, 415);
        assert_eq!(html.status, 415);
    }
}

mod m8 {
    use super::*;

    /// Java: `Api10UnsafeConsumptionOfApisTest.aFormulaInTheDataIsEscapedInTheExport`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_formula_in_the_data_is_escaped_in_the_export() {
        let world = World::start().await;
        let team = world.team().await;
        let id = world.expense(&team, &team.member, "DRAFT").await;
        sqlx::query("update expenses set merchant = $1, note = $2 where id = $3")
            .bind(r#"=HYPERLINK("http://evil.test","click me")"#)
            .bind("@SUM(1+1)")
            .bind(id)
            .execute(world.fixtures.pool())
            .await
            .unwrap();

        let csv = world
            .as_user(&team.approver)
            .get(&format!("{}/expenses.csv", team.path()))
            .send()
            .await
            .text();

        assert!(
            csv.contains(r#""'=HYPERLINK(""http://evil.test"",""click me"")""#),
            "{csv}"
        );
        assert!(csv.contains(",'@SUM(1+1)"), "{csv}");
        assert!(
            csv.lines()
                .skip(1)
                .all(|line| !line.contains(",=") && !line.contains(",@")),
            "{csv}"
        );
    }

    /// Java: `Api10UnsafeConsumptionOfApisTest.aFormulaThatEntersThroughAnImportIsStoredAsTypedAndEscapedOnTheWayOut`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_formula_that_enters_through_an_import_is_stored_as_typed_and_escaped_on_the_way_out() {
        let world = World::start().await;
        let team = world.team().await;
        let member = world.as_user(&team.member);
        let csv = "date,merchant,amount,currency,category,note\n2026-01-02,=cmd|' /C calc'!A0,5.00,EUR,MEALS,+1+1\n";

        let queued = member
            .post(&format!("{}/imports", team.path()))
            .file("file", "e.csv", "text/csv", csv)
            .send()
            .await;
        assert_eq!(queued.status, 202);
        let import = format!("{}/imports/{}", team.path(), queued.json()["id"].as_str().unwrap());
        eventually("the import to complete", Duration::from_secs(30), || async {
            (member.get(&import).send().await.json()["status"] == "COMPLETED").then_some(())
        })
        .await;

        // Stored as typed (nothing is rewritten on the way in) ...
        let stored: String = sqlx::query_scalar("select merchant from expenses where team_id = $1")
            .bind(team.id)
            .fetch_one(world.fixtures.pool())
            .await
            .unwrap();
        assert_eq!(stored, "=cmd|' /C calc'!A0");
        // ... and neutralised when it leaves.
        let exported = member.get(&format!("{}/expenses.csv", team.path())).send().await.text();
        assert!(exported.contains("'=cmd|' /C calc'!A0"), "{exported}");
        assert!(exported.contains(",'+1+1"), "{exported}");
    }

    /// Java: `Api10UnsafeConsumptionOfApisTest.aCsvThatIsReallyBinaryIsRefused`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_csv_that_is_really_binary_is_refused() {
        let world = World::start().await;
        let team = world.team().await;
        let member = world.as_user(&team.member);

        let import = member
            .post(&format!("{}/imports", team.path()))
            .file("file", "e.csv", "text/csv", EXE)
            .send()
            .await;
        let export = member.get(&format!("{}/expenses.csv", team.path())).send().await;

        assert_eq!(import.status, 415);
        assert_eq!(export.status, 200);
    }
}
