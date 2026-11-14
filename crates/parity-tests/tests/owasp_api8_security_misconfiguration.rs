//! OWASP API8:2023 Security Misconfiguration: security headers, an exact CORS allowlist, the actuator on its
//! own port, no docs outside local development, and errors that reveal nothing.
//!
//! Java: `src/test/java/com/ledgerly/owasp/Api8SecurityMisconfigurationTest.java`, which starts the app with
//! `ledgerly.security.allowed-origins=https://app.example.com`. This file runs in the `owasp` group with the
//! same setting.
//!
//! Not here (see `NOT_PARITY.md`): the 500 that reveals nothing, which needs a route that fails, and the
//! allowlist's unit test.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::http::Response;
use ledgerly_parity_tests::{V1, World};
use serde_json::json;

const ALLOWED_ORIGIN: &str = "https://app.example.com";

fn me() -> String {
    format!("{V1}/me")
}

mod m5 {
    use super::*;

    /// Java: `Api8SecurityMisconfigurationTest.actuatorIsNotReachableOnThePublicPort`.
    #[tokio::test]
    async fn the_actuator_is_not_reachable_on_the_public_port() {
        let world = World::start().await;
        let user = world.user().await;

        for path in ["/actuator/health", "/actuator/prometheus", "/actuator/env"] {
            assert_eq!(world.anonymous().get(path).send_unchecked().await.status, 401, "{path}");
            assert_eq!(
                world.as_user(&user).get(path).send_unchecked().await.status,
                404,
                "{path}"
            );
        }
    }

    /// Java: `Api8SecurityMisconfigurationTest.swaggerUiIsNotServedOutsideLocalDevelopment`.
    #[tokio::test]
    async fn swagger_ui_is_not_served_outside_local_development() {
        let world = World::start().await;
        let user = world.user().await;

        for path in [
            "/swagger-ui/index.html",
            "/swagger-ui/openapi.yaml",
            "/swagger-ui/swagger-ui-bundle.js",
        ] {
            assert_eq!(world.anonymous().get(path).send_unchecked().await.status, 401, "{path}");
            assert_eq!(
                world.as_user(&user).get(path).send_unchecked().await.status,
                404,
                "{path}"
            );
        }
    }

    /// Java: `Api8SecurityMisconfigurationTest.notFoundAndBadMethodsAreProblemsNotWhitelabelPages`.
    #[tokio::test]
    async fn not_found_and_bad_methods_are_problems_not_whitelabel_pages() {
        let world = World::start().await;
        let user = world.as_user(&world.user().await);

        let responses = [
            user.get(&format!("{V1}/does-not-exist")).send_unchecked().await,
            user.put(&me()).json(&json!({})).send_unchecked().await,
        ];

        for response in responses {
            assert!(
                response.header("content-type").unwrap().contains("problem+json"),
                "{:?}",
                response.header("content-type")
            );
            let text = response.text();
            assert!(!text.to_lowercase().contains("whitelabel"), "{text}");
            assert!(!text.contains("Tomcat"), "{text}");
            assert!(!text.contains("org.springframework"), "{text}");
        }
    }
}

mod m10 {
    use super::*;

    /// Java: `Api8SecurityMisconfigurationTest.everyResponseCarriesTheSecurityHeaders`.
    #[tokio::test]
    #[ignore = "M10"]
    async fn every_response_carries_the_security_headers() {
        let world = World::start().await;
        let user = world.user().await;

        let responses = [
            world.as_user(&user).get(&me()).send().await,
            world.anonymous().get(&me()).send().await,
            world
                .as_user(&user)
                .get(&format!("{V1}/nothing-here"))
                .send_unchecked()
                .await,
        ];

        for response in responses {
            let status = response.status;
            let header = |name: &str| {
                response
                    .header(name)
                    .unwrap_or_else(|| panic!("the {status} response has no {name}"))
            };
            let hsts = header("strict-transport-security");
            assert!(
                hsts.contains("max-age=31536000") && hsts.contains("includeSubDomains"),
                "{hsts}"
            );
            assert_eq!(
                header("content-security-policy"),
                "default-src 'none'; frame-ancestors 'none'"
            );
            assert_eq!(header("x-content-type-options"), "nosniff");
            assert_eq!(header("x-frame-options"), "DENY");
            assert_eq!(header("referrer-policy"), "no-referrer");
            assert!(header("cache-control").contains("no-store"));
        }
    }

    async fn preflight(world: &World, origin: &str, request_headers: Option<&str>) -> Response {
        let mut request = world
            .anonymous()
            .options(&me())
            .header("Origin", origin)
            .header("Access-Control-Request-Method", "GET");
        if let Some(headers) = request_headers {
            request = request.header("Access-Control-Request-Headers", headers);
        }
        // The spec describes no OPTIONS operation.
        request.send_unchecked().await
    }

    /// Java: `Api8SecurityMisconfigurationTest.anAllowlistedOriginMayCallFromABrowser`.
    #[tokio::test]
    #[ignore = "M10"]
    async fn an_allowlisted_origin_may_call_from_a_browser() {
        let world = World::start().await;
        let user = world.user().await;

        let preflight = preflight(&world, ALLOWED_ORIGIN, Some("authorization")).await;
        assert_eq!(preflight.status, 200);
        assert_eq!(preflight.header("access-control-allow-origin"), Some(ALLOWED_ORIGIN));
        assert_eq!(preflight.header("access-control-allow-credentials"), Some("true"));
        assert!(
            preflight
                .header("access-control-allow-headers")
                .unwrap()
                .to_lowercase()
                .contains("authorization")
        );

        let actual = world
            .as_user(&user)
            .get(&me())
            .header("Origin", ALLOWED_ORIGIN)
            .send()
            .await;
        assert_eq!(actual.status, 200);
        assert_eq!(actual.header("access-control-allow-origin"), Some(ALLOWED_ORIGIN));
        let exposed = actual.header("access-control-expose-headers").unwrap();
        assert!(exposed.contains("ETag") && exposed.contains("Retry-After"), "{exposed}");
    }

    /// Java: `Api8SecurityMisconfigurationTest.anyOtherOriginIsRefusedAndNeverGetsAWildcard`.
    #[tokio::test]
    #[ignore = "M10"]
    async fn any_other_origin_is_refused_and_never_gets_a_wildcard() {
        let world = World::start().await;
        let user = world.user().await;

        for origin in [
            "https://evil.example.com",
            "https://app.example.com.evil.test",
            "http://app.example.com",
            "null",
        ] {
            let preflight = preflight(&world, origin, None).await;
            assert_eq!(preflight.status, 403, "{origin}");
            assert_eq!(preflight.header("access-control-allow-origin"), None, "{origin}");

            // Spring's CORS filter answers before the handler, with a plain-text body the spec does not describe.
            let actual = world
                .as_user(&user)
                .get(&me())
                .header("Origin", origin)
                .send_unchecked()
                .await;
            assert_eq!(actual.status, 403, "{origin}");
            assert_eq!(actual.header("access-control-allow-origin"), None, "{origin}");
        }
    }

    /// Java: `Api8SecurityMisconfigurationTest.actuatorLivesOnItsOwnPortAndExposesOnlyHealthInfoAndPrometheus`.
    /// The part that needs no metrics is `operational.rs::m1`.
    #[tokio::test]
    #[ignore = "M10"]
    async fn the_actuator_lives_on_its_own_port_and_exposes_only_health_info_and_prometheus() {
        let world = World::start().await;
        let management = world.management();

        for exposed in ["/actuator/health", "/actuator/health/liveness", "/actuator/prometheus"] {
            assert_eq!(management.get(exposed).send_unchecked().await.status, 200, "{exposed}");
        }
        for hidden in [
            "/actuator/env",
            "/actuator/beans",
            "/actuator/heapdump",
            "/actuator/loggers",
            "/actuator/mappings",
            "/actuator/configprops",
        ] {
            assert_ne!(management.get(hidden).send_unchecked().await.status, 200, "{hidden}");
        }
    }
}
