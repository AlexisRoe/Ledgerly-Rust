//! A token is refused cleanly when the identity provider cannot be reached.
//!
//! Java: `ExternalIdentityTest.aTokenIsRefusedCleanlyWhenTheIdentityProviderCannotBeReached`, which configures
//! an issuer that does not exist. This file runs in the `keycloak-down` group, with the Keycloak container
//! stopped. Java also asserts a log line; it is checked through the app's log here.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::{V1, World};

/// Java's forged token: ES256 header with kid `old`, a subject, and a signature of zeros.
const FORGED: &str =
    "eyJhbGciOiJFUzI1NiIsImtpZCI6Im9sZCJ9.eyJzdWIiOiIxMTExMTExMS0xMTExLTExMTEtMTExMS0xMTExMTExMTExMTEifQ.AAAA";

mod m11 {
    use super::*;

    #[tokio::test]
    #[ignore = "M11"]
    async fn a_token_is_refused_cleanly_when_the_identity_provider_cannot_be_reached() {
        let world = World::start().await;

        let response = world.with_token(FORGED).get(&format!("{V1}/me")).send().await;

        assert_eq!(response.status, 401);
        assert!(
            response
                .header("content-type")
                .unwrap()
                .contains("application/problem+json")
        );
        assert!(response.text().contains("UNAUTHENTICATED"));
        assert!(!response.text().contains("Exception"));
        let logs = world.target.app_logs();
        assert!(logs.contains("the identity provider is not reachable"), "{logs}");
    }
}
