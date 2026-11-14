//! OWASP API4:2023 Unrestricted Resource Consumption: caps on batches, pages, uploads, bodies and imports, and
//! request rates per user and per client address.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api4UnrestrictedResourceConsumptionTest.java`, which starts the app
//! with 30 requests per minute per user and 12 per minute per address on the token-free endpoints. This file
//! runs in the `api4` group with the same limits.
//!
//! Not here, because they need a clock (see `NOT_PARITY.md`): the allowances being back after one and two
//! minutes.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::fixtures::PDF;
use ledgerly_parity_tests::{V1, World, expense_json};
use serde_json::json;

mod m7 {
    use super::*;

    /// Java: `Api4UnrestrictedResourceConsumptionTest.uploadsOverTenMegabytesAreA413`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn uploads_over_ten_megabytes_are_a_413() {
        let world = World::start().await;
        let team = world.team().await;
        let expense = world.expense(&team, &team.member, "DRAFT").await;
        // `Arrays.copyOf(PDF, 10 MiB + 1)`: the PDF, padded with zero bytes.
        let mut big = PDF.to_vec();
        big.resize(10 * 1024 * 1024 + 1, 0);

        let response = world
            .as_user(&team.member)
            .post(&format!("{}/receipts", team.expense_path(expense)))
            .file("file", "big.pdf", "application/pdf", big)
            .send()
            .await;

        assert_eq!(response.status, 413);
    }
}

mod m8 {
    use super::*;

    /// Java: `Api4UnrestrictedResourceConsumptionTest.aHundredAndOneItemBatchIsA400`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_hundred_and_one_item_batch_is_a_400() {
        let world = World::start().await;
        let team = world.team().await;
        let items = vec![expense_json("1.00", "x"); 101];

        let response = world
            .as_user(&team.member)
            .post(&format!("{}/expenses:batch", team.path()))
            .json(&json!({"items": items}))
            .send()
            .await;

        assert_eq!(response.status, 400);
    }

    /// Java: `Api4UnrestrictedResourceConsumptionTest.jsonBodiesOverOneMegabyteAreA413BeforeTheyAreParsed`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn json_bodies_over_one_megabyte_are_a_413_before_they_are_parsed() {
        let world = World::start().await;
        let team = world.team().await;
        // `pad` is not a property of an expense: a parser would answer 400, the size limit answers first.
        let huge = format!("{{\"pad\":\"{}\"}}", "x".repeat(2 * 1024 * 1024));

        let response = world
            .as_user(&team.member)
            .post(&format!("{}/expenses", team.path()))
            .body("application/json", huge)
            .send()
            .await;

        assert_eq!(response.status, 413);
    }

    /// Java: `Api4UnrestrictedResourceConsumptionTest.aSixthImportOfTheDayIsA429`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_sixth_import_of_the_day_is_a_429() {
        let world = World::start().await;
        let team = world.team().await;
        let csv = "date,merchant,amount,currency,category\n2026-01-01,Cafe,1.00,EUR,MEALS\n";
        let upload = || {
            world
                .as_user(&team.member)
                .post(&format!("{}/imports", team.path()))
                .file("file", "e.csv", "text/csv", csv)
                .send()
        };

        for _ in 0..5 {
            assert_eq!(upload().await.status, 202);
        }
        let sixth = upload().await;

        assert_eq!(sixth.status, 429);
        assert!(sixth.header("retry-after").is_some());
    }
}

mod m9 {
    use super::*;

    /// Java: `Api4UnrestrictedResourceConsumptionTest.aPageOfMoreThanHundredIsRefusedNotClamped`.
    #[tokio::test]
    #[ignore = "M9"]
    async fn a_page_of_more_than_hundred_is_refused_not_clamped() {
        let world = World::start().await;
        let team = world.team().await;

        let expenses = world
            .as_user(&team.member)
            .get(&format!("{}/expenses?size=101", team.path()))
            .send()
            .await;
        let audit = world
            .as_user(&team.owner)
            .get(&format!("{}/audit-events?limit=101", team.path()))
            .send()
            .await;

        assert_eq!(expenses.status, 400);
        assert_eq!(audit.status, 400);
    }
}

mod m10 {
    use super::*;

    /// Java: `Api4UnrestrictedResourceConsumptionTest.aBurstOfRequestsFromOneUserGets429WithRetryAfterWhileOthersAreUnaffected`,
    /// up to the point where Java moves its clock forward by a minute.
    #[tokio::test]
    #[ignore = "M10"]
    async fn a_burst_of_requests_from_one_user_gets_429_with_retry_after_while_others_are_unaffected() {
        let world = World::start().await;
        let (busy, calm) = (world.user().await, world.user().await);
        let me = format!("{V1}/me");

        let mut refused = Vec::new();
        for _ in 0..40 {
            let response = world.as_user(&busy).get(&me).send().await;
            if response.status == 429 {
                refused.push(response);
            }
        }

        // 30 allowed per minute, the other 10 refused.
        assert_eq!(refused.len(), 10);
        let last = refused.last().unwrap();
        let retry_after: u64 = last.header("retry-after").unwrap().parse().unwrap();
        assert!((1..=60).contains(&retry_after), "{retry_after}");
        assert_eq!(last.problem_code().as_deref(), Some("RATE_LIMITED"));
        assert_eq!(world.as_user(&calm).get(&me).send().await.status, 200);
    }

    /// Java: `Api4UnrestrictedResourceConsumptionTest.theEndpointsThatNeedNoTokenAreLimitedByClientAddress`, up
    /// to the point where Java moves its clock forward by two minutes.
    #[tokio::test]
    #[ignore = "M10"]
    async fn the_endpoints_that_need_no_token_are_limited_by_client_address() {
        let world = World::start().await;

        let mut refused = Vec::new();
        for _ in 0..20 {
            let response = world.login("nobody@example.com", "wrong-password-123").await;
            if response.status == 429 {
                refused.push(response);
            }
        }

        // 12 allowed per minute from this address.
        assert!(refused.len() >= 8, "{}", refused.len());
        assert!(refused.last().unwrap().header("retry-after").is_some());
    }
}
