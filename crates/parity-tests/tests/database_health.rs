//! `/actuator/health` includes the database, as Spring Boot's `db` indicator does; the probes do not.
//!
//! Java has no test for it: Spring Boot adds the indicator for any `DataSource` (decided after M2, plan M10.6).
//! The test stops Postgres itself and starts it again, so its file runs in a group of its own (`database-down`)
//! and only when `just parity` started the app.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::process::{Command, Stdio};
use std::time::Duration;

use ledgerly_parity_tests::{Target, eventually};
use serde_json::{Value, json};

/// `docker compose <action> postgres` in the workspace, where `compose.yaml` is.
fn postgres(action: &str) {
    let workspace = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let status = Command::new("docker")
        .args(["compose", action, "postgres"])
        .current_dir(workspace)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("docker can be started");
    assert!(status.success(), "docker compose {action} postgres failed");
}

/// Starts Postgres again however the test ends, so that the groups after this one find it.
struct Restart;

impl Drop for Restart {
    fn drop(&mut self) {
        postgres("start");
    }
}

async fn health(target: &Target, path: &str) -> (u16, Value) {
    let response = target.management().get(path).send_unchecked().await;
    (response.status, response.json())
}

mod m10 {
    use super::*;

    /// Plan M10.6: Postgres gone, `/actuator/health` is 503 `DOWN` while liveness and readiness stay `UP`; Postgres
    /// back, it is `UP` again.
    #[tokio::test]
    async fn health_turns_down_without_the_database_and_up_again() {
        let target = Target::from_env();
        assert_eq!(health(&target, "/actuator/health").await.0, 200);

        let restart = Restart;
        postgres("stop");
        // Each check waits for a connection as long as the pool does (30 s).
        let down = eventually("health to turn DOWN", Duration::from_secs(120), || async {
            let (status, body) = health(&target, "/actuator/health").await;
            (status == 503).then_some(body)
        })
        .await;
        assert_eq!(down, json!({"status": "DOWN", "groups": ["liveness", "readiness"]}));
        for probe in ["/actuator/health/liveness", "/actuator/health/readiness"] {
            assert_eq!(health(&target, probe).await, (200, json!({"status": "UP"})), "{probe}");
        }

        drop(restart);
        let up = eventually("health to be UP again", Duration::from_secs(120), || async {
            let (status, body) = health(&target, "/actuator/health").await;
            (status == 200).then_some(body)
        })
        .await;
        assert_eq!(up["status"], "UP");
    }
}
