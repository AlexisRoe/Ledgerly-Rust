//! The connection pool and the check that the schema is the one this build expects.
//!
//! Java counterpart: Spring Boot's `DataSource` auto-configuration with HikariCP, and Flyway's validation at
//! startup.

mod migrations;
mod schema;
#[cfg(feature = "testing")]
pub(crate) mod testing;
#[cfg(all(test, feature = "testing"))]
mod tls_tests;

use std::time::Duration;

use secrecy::ExposeSecret;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use sqlx::{Connection, PgConnection, PgPool};

pub use migrations::{VendoredMigration, flyway_checksum};
pub use schema::{Difference, HistoryRow, Mismatch, SchemaError, compare, verify};
#[cfg(feature = "testing")]
pub use testing::{TestDatabase, TestDatabaseError};

use crate::config::{DatabaseConfig, DatabaseUrl};

/// HikariCP's defaults, which Spring Boot leaves unchanged and Ledgerly-Java does not override.
const MAX_CONNECTIONS: u32 = 10;
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);

/// Tries one connection, then opens the pool.
///
/// The single attempt copies HikariCP's `initializationFailTimeout` of 1 ms: a database that cannot be reached
/// stops the start at once, as in Java. `PgPoolOptions::connect_with` would instead retry for the whole acquire
/// timeout (30 s) before giving up.
///
/// # Errors
///
/// The `sqlx` error of the first connection attempt. It names the host and user, never the password.
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    let options = connect_options(config);
    PgConnection::connect_with(&options).await?.close().await?;
    Ok(PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .idle_timeout(IDLE_TIMEOUT)
        .max_lifetime(MAX_LIFETIME)
        .connect_lazy_with(options))
}

/// Where and as whom to connect, taken only from the `SPRING_DATASOURCE_*` settings.
///
/// `PgConnectOptions::new` would also read `PGHOST`, `PGPASSWORD`, `~/.pgpass` and the like. Those are not
/// Java's names (ADR 0013), so they are not read.
#[must_use]
pub fn connect_options(config: &DatabaseConfig) -> PgConnectOptions {
    let url = &config.url;
    let mut options = PgConnectOptions::new_without_pgpass()
        .host(url.host())
        .port(url.port())
        .database(url.database())
        .username(&config.username)
        .password(config.password.expose_secret())
        .ssl_mode(ssl_mode(url));
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    if let Some(root) = ssl_root_cert(url, home.as_deref()) {
        options = options.ssl_root_cert(root);
    }
    if let Some(name) = url.parameter("ApplicationName") {
        options = options.application_name(name);
    }
    options
}

/// pgjdbc's `sslmode`, whose default is `prefer`, as is `sqlx`'s (decided after M11.4):
///
/// - `disable`, `allow`, `prefer` and `require` as in pgjdbc: `require` encrypts but checks no certificate,
///   `prefer` encrypts if the server can, `allow` only if the server insists.
/// - `verify-full`: the certificate must chain to a trusted root and name the host.
/// - `verify-ca` is checked like `verify-full`, stricter than pgjdbc, which leaves out the host name. sqlx
///   trusts the system's CA certificates besides `sslrootcert` (pgjdbc trusts only that file), and with them a
///   check without the host name would accept any publicly trusted certificate.
fn ssl_mode(url: &DatabaseUrl) -> PgSslMode {
    match url.parameter("sslmode") {
        Some("disable") => PgSslMode::Disable,
        Some("allow") => PgSslMode::Allow,
        Some("require") => PgSslMode::Require,
        Some("verify-ca" | "verify-full") => PgSslMode::VerifyFull,
        _ => PgSslMode::Prefer,
    }
}

/// The root certificate a `verify-*` connection trusts besides the system's: `sslrootcert`, or as in pgjdbc
/// `~/.postgresql/root.crt` when that file exists. Other modes check no certificate, so they need none.
fn ssl_root_cert(url: &DatabaseUrl, home: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    if !matches!(ssl_mode(url), PgSslMode::VerifyFull) {
        return None;
    }
    if let Some(path) = url.parameter("sslrootcert") {
        return Some(path.into());
    }
    let default = home?.join(".postgresql").join("root.crt");
    default.is_file().then_some(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(parameters: &str) -> DatabaseUrl {
        DatabaseUrl::parse(&format!("jdbc:postgresql://db/ledgerly?{parameters}")).unwrap()
    }

    #[test]
    fn the_ssl_modes_are_pgjdbcs_with_verify_ca_checked_like_verify_full() {
        for (parameters, mode) in [
            ("", PgSslMode::Prefer),
            ("sslmode=disable", PgSslMode::Disable),
            ("sslmode=allow", PgSslMode::Allow),
            ("sslmode=prefer", PgSslMode::Prefer),
            ("sslmode=require", PgSslMode::Require),
            ("sslmode=verify-ca", PgSslMode::VerifyFull),
            ("sslmode=verify-full", PgSslMode::VerifyFull),
        ] {
            assert_eq!(
                format!("{:?}", ssl_mode(&url(parameters))),
                format!("{mode:?}"),
                "{parameters}"
            );
        }
    }

    /// pgjdbc reads `~/.postgresql/root.crt` when `sslrootcert` is not given.
    #[test]
    fn the_root_certificate_is_sslrootcert_or_the_one_in_the_home_directory() {
        let home = tempfile::tempdir().unwrap();
        let default = home.path().join(".postgresql").join("root.crt");

        assert_eq!(ssl_root_cert(&url("sslmode=verify-full"), Some(home.path())), None);
        std::fs::create_dir_all(default.parent().unwrap()).unwrap();
        std::fs::write(&default, "").unwrap();
        assert_eq!(
            ssl_root_cert(&url("sslmode=verify-full"), Some(home.path())),
            Some(default)
        );
        assert_eq!(
            ssl_root_cert(&url("sslmode=verify-ca&sslrootcert=/certs/ca.crt"), Some(home.path())),
            Some("/certs/ca.crt".into())
        );
        assert_eq!(
            ssl_root_cert(&url("sslmode=require&sslrootcert=/certs/ca.crt"), Some(home.path())),
            None,
            "require checks no certificate"
        );
    }
}
