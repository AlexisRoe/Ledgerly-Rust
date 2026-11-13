//! What the `local` profile serves that no other profile does: Swagger UI and `/docs`.
//!
//! Java: `src/test/java/com/ledgerly/SwaggerUiProfileTest.java` (`Local`, `LocalDocsShortcut`), which runs with
//! the profiles `local,test`. This file runs in the `local` group. The `NotLocal` half is in
//! `docs_outside_local.rs`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::World;

mod m3 {
    use super::*;

    /// Java: `SwaggerUiProfileTest.Local.servesUiAndSpec`.
    #[tokio::test]
    async fn serves_ui_and_spec() {
        let world = World::start().await;

        for path in [
            "/swagger-ui/index.html",
            "/swagger-ui/openapi.yaml",
            "/swagger-ui/swagger-ui-bundle.js",
        ] {
            assert_eq!(world.anonymous().get(path).send_unchecked().await.status, 200, "{path}");
        }
    }

    /// Java: `SwaggerUiProfileTest.LocalDocsShortcut.docsLeadsToTheUiWithoutAToken`. Java's client follows the
    /// redirect; this one does not, so it follows it once by hand.
    #[tokio::test]
    async fn docs_leads_to_the_ui_without_a_token() {
        let world = World::start().await;

        let redirect = world.anonymous().get("/docs").send_unchecked().await;
        assert!((300..400).contains(&redirect.status), "{}", redirect.status);
        let location = redirect.header("location").unwrap().to_owned();
        let path = location
            .split_once("://")
            .map_or(location.as_str(), |(_, rest)| &rest[rest.find('/').unwrap()..]);
        let page = world.anonymous().get(path).send_unchecked().await;

        assert_eq!(page.status, 200);
        assert!(
            page.text().to_lowercase().contains("swagger"),
            "followed the redirect to the UI page"
        );
    }
}

mod m5 {
    use super::*;

    /// Java: `SwaggerUiProfileTest.LocalDocsShortcut.onlyReadingIsOpen`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn only_reading_is_open() {
        let world = World::start().await;

        assert_eq!(world.anonymous().post("/docs").send_unchecked().await.status, 401);
    }
}
