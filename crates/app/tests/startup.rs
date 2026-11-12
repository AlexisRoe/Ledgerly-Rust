//! What the `ledgerly` binary does before it serves anything.
//!
//! Java counterpart: `ProductionEnvironmentCheckTest.theWholeApplicationRefusesToStartInProdWithoutConfiguration`.

// clippy.toml exempts `#[test]` functions only; the helpers of an integration test are test code too.
#![allow(clippy::unwrap_used)]

use std::process::Command;

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
fn the_binary_serves_until_sigterm_and_then_exits_cleanly() {
    let (api_port, management_port) = (free_port(), free_port());
    let mut child = Command::new(env!("CARGO_BIN_EXE_ledgerly"))
        .env_clear()
        .env("SPRING_PROFILES_ACTIVE", "local")
        .env("SPRING_DATASOURCE_URL", "jdbc:postgresql://localhost:5432/ledgerly")
        .env("SPRING_DATASOURCE_USERNAME", "ledgerly")
        .env("SPRING_DATASOURCE_PASSWORD", "ledgerly")
        .env("SERVER_PORT", api_port.to_string())
        .env("MANAGEMENT_SERVER_PORT", management_port.to_string())
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

    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0), "SIGTERM ends the process with exit code 0");
    assert_eq!(health_status(management_port), None);
}
