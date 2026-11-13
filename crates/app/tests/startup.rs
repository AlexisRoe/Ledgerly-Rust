//! What the `ledgerly` binary does before it serves anything.
//!
//! Java counterpart: `ProductionEnvironmentCheckTest.theWholeApplicationRefusesToStartInProdWithoutConfiguration`.
//! The schema check (plan M2.1) has none: Java migrates at startup instead.

// clippy.toml exempts `#[test]` functions only; the helpers of an integration test are test code too.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::process::Command;

use ledgerly_shared::database::TestDatabase;

/// A test database, and the runtime its pool lives on, for tests that drive the binary synchronously.
fn test_database() -> (tokio::runtime::Runtime, TestDatabase) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let database = runtime.block_on(TestDatabase::new()).unwrap();
    (runtime, database)
}

/// The `ledgerly` binary in the `test` profile with every required setting, pointed at `database`.
fn ledgerly(database: &TestDatabase) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ledgerly"));
    command
        .env_clear()
        .env("SPRING_PROFILES_ACTIVE", "test")
        .env("SPRING_DATASOURCE_URL", database.jdbc_url())
        .env("SPRING_DATASOURCE_USERNAME", TestDatabase::USERNAME)
        .env("SPRING_DATASOURCE_PASSWORD", TestDatabase::PASSWORD)
        .env("LEDGERLY_STORAGE_S3_ENDPOINT", "http://localhost:8333")
        .env("LEDGERLY_STORAGE_S3_BUCKET", "test-files")
        .env("LEDGERLY_STORAGE_S3_ACCESS_KEY", "key-2b8e")
        .env("LEDGERLY_STORAGE_S3_SECRET_KEY", "secret-9d3f")
        .env("LEDGERLY_RECEIPTS_CLAMAV_HOST", "localhost")
        .env("LEDGERLY_RECEIPTS_CLAMAV_PORT", "3310");
    command
}

#[test]
fn prod_without_configuration_exits_non_zero_and_lists_every_missing_variable() {
    let output = Command::new(env!("CARGO_BIN_EXE_ledgerly"))
        .env_clear()
        .env("SPRING_PROFILES_ACTIVE", "prod")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("APPLICATION FAILED TO START"), "{stderr}");
    for variable in [
        "SPRING_DATASOURCE_URL",
        "SPRING_DATASOURCE_USERNAME",
        "SPRING_DATASOURCE_PASSWORD",
        "LEDGERLY_AUTH_JWT_PRIVATE_KEY",
        "LEDGERLY_AUTH_JWT_PUBLIC_KEY",
        "LEDGERLY_STORAGE_S3_ENDPOINT",
        "LEDGERLY_STORAGE_S3_BUCKET",
        "LEDGERLY_STORAGE_S3_ACCESS_KEY",
        "LEDGERLY_STORAGE_S3_SECRET_KEY",
        "LEDGERLY_RECEIPTS_CLAMAV_HOST",
        "LEDGERLY_RECEIPTS_CLAMAV_PORT",
    ] {
        assert!(
            stderr.contains(&format!("(environment variable {variable})")),
            "{variable} is not listed in:\n{stderr}"
        );
    }
}

#[test]
fn a_secret_in_the_environment_never_reaches_the_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_ledgerly"))
        .env_clear()
        .env("SPRING_PROFILES_ACTIVE", "prod")
        .env("SPRING_DATASOURCE_URL", "postgres://admin:hunter2@db/ledgerly")
        .env("SPRING_DATASOURCE_PASSWORD", "pw-4f9a")
        .env("LEDGERLY_STORAGE_S3_SECRET_KEY", "secret-9d3f")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for secret in ["hunter2", "pw-4f9a", "secret-9d3f"] {
        assert!(!printed.contains(secret), "{secret} appears in:\n{printed}");
    }
}

/// A port that was free a moment ago. The binary cannot report a port the operating system picked until it
/// logs (M1.4), so the test picks one itself.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn health_status(port: u16) -> Option<u16> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .write_all(b"GET /actuator/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    response.split_whitespace().nth(1)?.parse().ok()
}

#[test]
fn the_binary_logs_ecs_json_serves_until_sigterm_and_then_exits_cleanly() {
    let (_runtime, database) = test_database();
    let (api_port, management_port) = (free_port(), free_port());
    let child = ledgerly(&database)
        .env("SERVER_PORT", api_port.to_string())
        .env("MANAGEMENT_SERVER_PORT", management_port.to_string())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while health_status(management_port) != Some(200) {
        assert!(std::time::Instant::now() < deadline, "the binary never became healthy");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // `kill` instead of a signal crate: sending a signal from Rust needs `unsafe` or a new dependency.
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());

    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "SIGTERM ends the process with exit code 0"
    );
    assert_eq!(health_status(management_port), None);

    // Java: `LedgerlyApplicationTest` asserts that the log lines are JSON with a `message` member.
    let stdout = String::from_utf8(output.stdout).unwrap();
    let messages: Vec<String> = stdout
        .lines()
        .map(|line| {
            let json: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|_| panic!("not a JSON log line: {line}"));
            assert_eq!(json["service"]["name"], "ledgerly");
            json["message"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(
        messages,
        [
            "Started Ledgerly",
            "Commencing graceful shutdown. Waiting for active requests to complete"
        ]
    );
    for secret in ["key-2b8e", "secret-9d3f"] {
        assert!(!stdout.contains(secret));
    }
}

#[test]
fn a_database_without_v10_stops_the_start_and_names_the_version() {
    let (runtime, database) = test_database();
    runtime
        .block_on(sqlx::query("delete from flyway_schema_history where version = '10'").execute(database.pool()))
        .unwrap();
    let (api_port, management_port) = (free_port(), free_port());

    let output = ledgerly(&database)
        .env("SERVER_PORT", api_port.to_string())
        .env("MANAGEMENT_SERVER_PORT", management_port.to_string())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("APPLICATION FAILED TO START"), "{stderr}");
    assert!(
        stderr.contains("V10 (external identity) has not been applied"),
        "{stderr}"
    );
    // It stopped before it opened a port.
    assert_eq!(health_status(management_port), None);
}

#[test]
fn a_database_that_cannot_be_reached_stops_the_start_without_printing_the_password() {
    let output = Command::new(env!("CARGO_BIN_EXE_ledgerly"))
        .env_clear()
        .env("SPRING_PROFILES_ACTIVE", "local")
        // Nothing listens on port 1.
        .env("SPRING_DATASOURCE_URL", "jdbc:postgresql://127.0.0.1:1/ledgerly")
        .env("SPRING_DATASOURCE_USERNAME", "ledgerly")
        .env("SPRING_DATASOURCE_PASSWORD", "pw-4f9a")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Failed to connect to the database"), "{stderr}");
    assert!(!stderr.contains("pw-4f9a"), "{stderr}");
}
