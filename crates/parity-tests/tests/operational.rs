//! The operational surface the spec does not describe (inventory §4.6): the management port and its probes.
//!
//! Java: `src/test/java/com/ledgerly/ActuatorHealthTest.java`, and the management-port half of
//! `owasp/Api8SecurityMisconfigurationTest.java`.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use ledgerly_parity_tests::Target;
use serde_json::json;

mod m1 {
    use super::*;

    /// Java: `ActuatorHealthTest.healthIsUpOnManagementPort`.
    #[tokio::test]
    async fn health_is_up_on_the_management_port() {
        let target = Target::from_env();

        let response = target.management().get("/actuator/health").send_unchecked().await;

        assert_eq!(response.status, 200);
        // Java asserts this exact text, so there is no space after the colon.
        assert!(response.text().contains("\"status\":\"UP\""), "{}", response.text());
    }

    /// Inventory §4.6: `/actuator/health/liveness` and `/readiness` (`management.endpoint.health.probes`).
    #[tokio::test]
    async fn the_liveness_and_readiness_probes_are_up() {
        let target = Target::from_env();

        for path in ["/actuator/health/liveness", "/actuator/health/readiness"] {
            let response = target.management().get(path).send_unchecked().await;
            assert_eq!(response.status, 200, "{path}");
            assert_eq!(response.json(), json!({"status": "UP"}), "{path}");
        }
    }

    /// Inventory §4.6: `/actuator/info` is exposed (`exposure.include: health,info,prometheus`).
    #[tokio::test]
    async fn info_is_served_as_a_json_object() {
        let target = Target::from_env();

        let response = target.management().get("/actuator/info").send_unchecked().await;

        assert_eq!(response.status, 200);
        assert!(response.json().is_object());
    }

    /// Java: `Api8SecurityMisconfigurationTest.actuatorLivesOnItsOwnPortAndExposesOnlyHealthInfoAndPrometheus`,
    /// the part that needs no metrics: the endpoints that are not exposed do not answer 200.
    #[tokio::test]
    async fn endpoints_that_are_not_exposed_are_not_served_on_the_management_port() {
        let target = Target::from_env();

        for endpoint in ["env", "beans", "heapdump", "loggers", "mappings", "configprops"] {
            let response = target
                .management()
                .get(&format!("/actuator/{endpoint}"))
                .send_unchecked()
                .await;
            assert_ne!(response.status, 200, "{endpoint}");
        }
    }

    /// Plan M1.3: an unknown path on the management port is a 404.
    #[tokio::test]
    async fn an_unknown_path_on_the_management_port_is_404() {
        let target = Target::from_env();

        for path in ["/actuator/does-not-exist", "/does-not-exist"] {
            let response = target.management().get(path).send_unchecked().await;
            assert_eq!(response.status, 404, "{path}");
        }
    }
}
