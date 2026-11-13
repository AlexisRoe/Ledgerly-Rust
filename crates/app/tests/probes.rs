//! The running service seen from outside: two listeners, Spring Boot's probes, graceful shutdown.
//!
//! Java counterpart: `ActuatorHealthTest` and the management settings in `application.yaml`.

// clippy.toml exempts `#[test]` functions only; the helpers of an integration test are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use ledgerly_app::App;
use ledgerly_app::config::Config;
use ledgerly_app::telemetry::Telemetry;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const ACTUATOR_V3: &str = "application/vnd.spring-boot.actuator.v3+json";

struct Running {
    api: SocketAddr,
    management: SocketAddr,
    stop: oneshot::Sender<()>,
    served: JoinHandle<std::io::Result<()>>,
}

/// Starts the app on ports the operating system picks, as Java's test does with `management.server.port=0`.
async fn start() -> Running {
    let environment = HashMap::from([
        ("SPRING_PROFILES_ACTIVE", "local"),
        ("SPRING_DATASOURCE_URL", "jdbc:postgresql://localhost:5432/ledgerly"),
        ("SPRING_DATASOURCE_USERNAME", "ledgerly"),
        ("SPRING_DATASOURCE_PASSWORD", "ledgerly"),
        ("SERVER_PORT", "0"),
        ("MANAGEMENT_SERVER_PORT", "0"),
    ]);
    let config = Config::from_environment(&environment).unwrap();
    let telemetry = Telemetry::new(&config.tracing).unwrap();
    // The probes never touch the database: a pool that has not connected is enough.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://ledgerly@127.0.0.1:9/ledgerly")
        .unwrap();
    let app = App::bind(config, telemetry, pool).await.unwrap();
    let port = |address: SocketAddr| SocketAddr::from(([127, 0, 0, 1], address.port()));
    let api = port(app.api_address().unwrap());
    let management = port(app.management_address().unwrap());
    let (stop, stopped) = oneshot::channel::<()>();
    let served = tokio::spawn(app.serve(async move {
        let _ = stopped.await;
    }));
    Running {
        api,
        management,
        stop,
        served,
    }
}

async fn get(address: SocketAddr, path: &str) -> (u16, Option<String>, String) {
    let response = reqwest::get(format!("http://{address}{path}")).await.unwrap();
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|value| value.to_str().unwrap().to_owned());
    (status, content_type, response.text().await.unwrap())
}

async fn get_json(address: SocketAddr, path: &str) -> (u16, Option<String>, Value) {
    let (status, content_type, body) = get(address, path).await;
    (status, content_type, serde_json::from_str(&body).unwrap())
}

#[tokio::test]
async fn health_is_up_on_the_management_port() {
    let app = start().await;

    let (status, content_type, body) = get(app.management, "/actuator/health").await;

    assert_eq!(status, 200);
    assert_eq!(content_type.as_deref(), Some(ACTUATOR_V3));
    // Java's `ActuatorHealthTest` looks for exactly this text, so there is no space after the colon.
    assert!(body.contains("\"status\":\"UP\""), "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"status": "UP", "groups": ["liveness", "readiness"]})
    );
}

#[tokio::test]
async fn liveness_and_readiness_are_up() {
    let app = start().await;

    for path in ["/actuator/health/liveness", "/actuator/health/readiness"] {
        let (status, content_type, body) = get_json(app.management, path).await;
        assert_eq!((status, content_type.as_deref()), (200, Some(ACTUATOR_V3)), "{path}");
        assert_eq!(body, json!({"status": "UP"}), "{path}");
    }
}

#[tokio::test]
async fn info_reports_the_build() {
    let app = start().await;

    let (status, content_type, body) = get_json(app.management, "/actuator/info").await;

    assert_eq!((status, content_type.as_deref()), (200, Some(ACTUATOR_V3)));
    assert_eq!(body["build"]["artifact"], "ledgerly");
    assert_eq!(body["build"]["name"], "ledgerly");
    assert_eq!(body["build"]["group"], "com.ledgerly");
    assert_eq!(body["build"]["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn an_unknown_management_path_is_404() {
    let app = start().await;

    for path in [
        "/",
        "/actuator",
        "/actuator/env",
        "/actuator/beans",
        "/actuator/heapdump",
        "/actuator/loggers",
        "/actuator/mappings",
        "/actuator/configprops",
        "/actuator/health/db",
        "/actuator/health/",
        "/health",
    ] {
        let (status, content_type, body) = get_json(app.management, path).await;
        assert_eq!(status, 404, "{path}");
        assert_eq!(content_type.as_deref(), Some("application/json"), "{path}");
        assert_eq!(body["status"], 404, "{path}");
        assert_eq!(body["error"], "Not Found", "{path}");
        assert_eq!(body["path"], path);
        assert!(body["timestamp"].is_string(), "{path}");
    }
}

#[tokio::test]
async fn probes_only_answer_get() {
    let app = start().await;

    let response = reqwest::Client::new()
        .post(format!("http://{}/actuator/health", app.management))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 405);
}

/// On the API port the actuator paths are unknown, and like every unknown path they are 401 without a token
/// (Java: `Api8SecurityMisconfigurationTest.actuatorIsNotReachableOnThePublicPort`; the 404 with a token is in
/// the parity suite).
#[tokio::test]
async fn the_actuator_is_not_served_on_the_api_port_and_the_api_not_on_the_management_port() {
    let app = start().await;

    for path in ["/actuator/health", "/actuator/info", "/actuator/health/liveness"] {
        assert_eq!(get(app.api, path).await.0, 401, "{path}");
    }
    assert_ne!(app.api.port(), app.management.port());
}

#[tokio::test]
async fn stopping_shuts_both_listeners_down() {
    let app = start().await;
    assert_eq!(get(app.management, "/actuator/health").await.0, 200);

    app.stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), app.served)
        .await
        .expect("shutdown finishes without requests in flight")
        .unwrap()
        .unwrap();

    for address in [app.api, app.management] {
        assert!(reqwest::get(format!("http://{address}/actuator/health")).await.is_err());
    }
}
