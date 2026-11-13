//! The custom metrics count what they say, and are served on the management port only.
//!
//! Java: `src/test/java/com/ledgerly/owasp/MetricsTest.java`, which reads Micrometer's registry. From outside,
//! the registry is what `/actuator/prometheus` serves, under Micrometer's Prometheus names
//! (`ledgerly.expenses.transitions` becomes `ledgerly_expenses_transitions_total`).
//!
//! The tests compare a counter before and after, so this file has the app to itself (group `metrics`) and
//! runs one test at a time (`.config/nextest.toml`).

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::time::Duration;

use ledgerly_parity_tests::fixtures::infected_pdf;
use ledgerly_parity_tests::prometheus::sample;
use ledgerly_parity_tests::{World, eventually};

async fn scrape(world: &World) -> String {
    let response = world.management().get("/actuator/prometheus").send_unchecked().await;
    assert_eq!(response.status, 200);
    response.text()
}

async fn counter(world: &World, name: &str, labels: &[(&str, &str)]) -> f64 {
    sample(&scrape(world).await, name, labels)
}

const TRANSITIONS: &str = "ledgerly_expenses_transitions_total";
const QUARANTINED: &str = "ledgerly_receipts_quarantined_total";
const SKIPPED: &str = "ledgerly_imports_rows_skipped_total";

mod m10 {
    use super::*;

    /// Java: `MetricsTest.successfulTransitionsAreCountedByAction`.
    #[tokio::test]
    #[ignore = "M10"]
    async fn successful_transitions_are_counted_by_action() {
        let world = World::start().await;
        let team = world.team().await;
        let submit_before = counter(&world, TRANSITIONS, &[("action", "submit")]).await;
        let approve_before = counter(&world, TRANSITIONS, &[("action", "approve")]).await;
        let id = world.expense(&team, &team.member, "DRAFT").await;
        world.fixtures.accepted_receipt(team.id, id, &team.member.user).await;
        let base = team.expense_path(id);
        let approver = world.as_user(&team.approver);

        assert_eq!(
            world
                .as_user(&team.member)
                .post(&format!("{base}:submit"))
                .send()
                .await
                .status,
            200
        );
        assert_eq!(approver.post(&format!("{base}:approve")).send().await.status, 200);
        // A refused transition is not counted.
        assert_eq!(approver.post(&format!("{base}:approve")).send().await.status, 409);

        assert_eq!(
            counter(&world, TRANSITIONS, &[("action", "submit")]).await,
            submit_before + 1.0
        );
        assert_eq!(
            counter(&world, TRANSITIONS, &[("action", "approve")]).await,
            approve_before + 1.0
        );
    }

    /// Java: `MetricsTest.quarantinedReceiptsAreCounted`.
    #[tokio::test]
    #[ignore = "M10"]
    async fn quarantined_receipts_are_counted() {
        let world = World::start().await;
        let team = world.team().await;
        let id = world.expense(&team, &team.member, "DRAFT").await;
        let before = counter(&world, QUARANTINED, &[]).await;

        world
            .as_user(&team.member)
            .post(&format!("{}/receipts", team.expense_path(id)))
            .file("file", "r.pdf", "application/pdf", infected_pdf())
            .send()
            .await;

        eventually(
            "the quarantine counter to go up by one",
            Duration::from_secs(20),
            || async { (counter(&world, QUARANTINED, &[]).await == before + 1.0).then_some(()) },
        )
        .await;
    }

    /// Java: `MetricsTest.skippedImportRowsAreCounted`.
    #[tokio::test]
    #[ignore = "M10"]
    async fn skipped_import_rows_are_counted() {
        let world = World::start().await;
        let team = world.team().await;
        let member = world.as_user(&team.member);
        let before = counter(&world, SKIPPED, &[]).await;
        let csv = "date,merchant,amount,currency,category\n\
                   2026-01-01,Ok,1.00,EUR,MEALS\n\
                   2026-01-02,Bad,abc,EUR,MEALS\n\
                   2026-01-03,Worse,1.00,ZZZ,MEALS\n";

        let queued = member
            .post(&format!("{}/imports", team.path()))
            .file("file", "e.csv", "text/csv", csv)
            .send()
            .await
            .json();

        eventually(
            "the skipped-rows counter to go up by two",
            Duration::from_secs(30),
            || async { (counter(&world, SKIPPED, &[]).await == before + 2.0).then_some(()) },
        )
        .await;
        let import = member
            .get(&format!("{}/imports/{}", team.path(), queued["id"].as_str().unwrap()))
            .send()
            .await
            .json();
        assert_eq!(import["skippedRows"], 2);
    }

    /// Java: `MetricsTest.prometheusExposesThemOnTheManagementPortOnly`.
    #[tokio::test]
    #[ignore = "M10"]
    async fn prometheus_exposes_them_on_the_management_port_only() {
        let world = World::start().await;
        let team = world.team().await;
        let id = world.expense(&team, &team.member, "DRAFT").await;
        world.fixtures.accepted_receipt(team.id, id, &team.member.user).await;
        world
            .as_user(&team.member)
            .post(&format!("{}:submit", team.expense_path(id)))
            .send()
            .await;

        let body = scrape(&world).await;

        assert!(body.contains("ledgerly_expenses_transitions_total{action=\"submit\""));
        // Names of the JVM and of Spring MVC, which Java's test asserts as they are.
        assert!(body.contains("jvm_memory_used_bytes"));
        assert!(body.contains("http_server_requests"));
        // Metric tags hold no ids or names: the series count must stay small.
        assert!(!body.contains(team.member.email()));
        assert!(!body.contains(&team.id.to_string()));
        let public = world
            .as_user(&team.owner)
            .get("/actuator/prometheus")
            .send_unchecked()
            .await;
        assert_eq!(public.status, 404);
    }
}
