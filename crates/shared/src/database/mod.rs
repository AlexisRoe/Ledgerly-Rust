//! The connection pool and the check that the schema is the one this build expects.
//!
//! Java counterpart: Spring Boot's `DataSource` auto-configuration with HikariCP, and Flyway's validation at
//! startup.

mod migrations;
mod schema;
#[cfg(feature = "testing")]
mod testing;

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
    if let Some(name) = url.parameter("ApplicationName") {
        options = options.application_name(name);
    }
    options
}

/// The JDBC driver's default is `prefer`, as is `sqlx`'s. [`DatabaseUrl::parse`] has already refused every
/// mode that needs TLS.
fn ssl_mode(url: &DatabaseUrl) -> PgSslMode {
    match url.parameter("sslmode") {
        Some("disable") => PgSslMode::Disable,
        Some("allow") => PgSslMode::Allow,
        _ => PgSslMode::Prefer,
    }
}
