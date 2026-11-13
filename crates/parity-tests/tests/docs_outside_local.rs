//! Outside the `local` profile there is no Swagger UI and no `/docs`, and they are behind the token check
//! like every other unknown path.
//!
//! Java: `src/test/java/com/ledgerly/SwaggerUiProfileTest.java` (`NotLocal`).

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::World;

mod m5 {
    use super::*;

    /// Java: `SwaggerUiProfileTest.NotLocal.isDeniedByDefault`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn is_denied_by_default() {
        let world = World::start().await;

        assert_eq!(
            world
                .anonymous()
                .get("/swagger-ui/index.html")
                .send_unchecked()
                .await
                .status,
            401
        );
    }

    /// Java: `SwaggerUiProfileTest.NotLocal.docsIsDeniedToo`.
    #[tokio::test]
    #[ignore = "M5"]
    async fn docs_is_denied_too() {
        let world = World::start().await;

        assert_eq!(world.anonymous().get("/docs").send_unchecked().await.status, 401);
    }
}
