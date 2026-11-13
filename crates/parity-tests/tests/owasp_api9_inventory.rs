//! OWASP API9:2023 Improper Inventory Management: the OpenAPI file is the inventory, and the application
//! serves exactly what it lists.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api9ImproperInventoryManagementTest.java`, which runs with the
//! profiles `local,test`. This file runs in the `local` group.
//!
//! Java compares its route table with the spec. From outside there is no route table, so the first test
//! checks the half that can be seen: every documented operation is served. That nothing undocumented is
//! served is proven in-process (see `NOT_PARITY.md`).

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::World;
use ledgerly_parity_tests::spec::Spec;
use reqwest::Method;
use serde_json::json;
use uuid::Uuid;

/// The vendored spec, byte for byte.
const SPEC_FILE: &str = include_str!("../../../api/openapi.yaml");

mod m3 {
    use super::*;

    /// Java: `Api9ImproperInventoryManagementTest.theSpecTheApplicationServesIsTheFileInTheRepository`.
    #[tokio::test]
    #[ignore = "M3"]
    async fn the_spec_the_application_serves_is_the_file_in_the_repository() {
        let world = World::start().await;

        let served = world.anonymous().get("/swagger-ui/openapi.yaml").send_unchecked().await;

        assert_eq!(served.status, 200);
        assert!(
            served.text() == SPEC_FILE,
            "the served spec differs from api/openapi.yaml"
        );
        assert!(served.text().lines().any(|line| line == "openapi: 3.1.0"));
    }
}

mod m9 {
    use super::*;

    /// Java: `Api9ImproperInventoryManagementTest.everyServedEndpointIsInTheSpecAndEveryDocumentedOneIsServed`,
    /// the direction "documented, therefore served". A route that does not exist answers 404 with the code
    /// `REQUEST_REJECTED`, and a path that exists under another method answers 405; a served operation gives
    /// neither, whatever else it thinks of an empty request with made-up ids.
    #[tokio::test]
    #[ignore = "M9"]
    async fn every_documented_operation_is_served() {
        let world = World::start().await;
        let user = world.as_user(&world.user().await);
        let operations: Vec<(&str, &str)> = Spec::vendored().operations().collect();
        assert!(operations.len() > 30, "{}", operations.len());

        for (method, template) in operations {
            let path = template
                .split('/')
                .map(|segment| match segment.split_once('}') {
                    Some((_, suffix)) if segment.starts_with('{') => format!("{}{suffix}", Uuid::new_v4()),
                    _ => segment.to_owned(),
                })
                .collect::<Vec<_>>()
                .join("/");
            let mut request = user.request(Method::from_bytes(method.as_bytes()).unwrap(), &path);
            if method != "GET" && method != "DELETE" {
                request = request.json(&json!({}));
            }

            let response = request.send_unchecked().await;

            assert_ne!(response.status, 405, "{method} {template} is documented but not served");
            assert!(
                !(response.status == 404 && response.problem_code().as_deref() == Some("REQUEST_REJECTED")),
                "{method} {template} is documented but not served"
            );
        }
    }
}
