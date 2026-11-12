//! What the `ledgerly` binary does before it serves anything.
//!
//! Java counterpart: `ProductionEnvironmentCheckTest.theWholeApplicationRefusesToStartInProdWithoutConfiguration`.

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
