//! The service's configuration as typed values, and the parsers for the forms Spring Boot accepts.
//!
//! The names, defaults and profiles are Ledgerly-Java's (ADR 0013; `application*.yaml` and the
//! `@ConfigurationProperties` records). This module only describes the values. Reading them from the
//! environment is the `app` crate's job.

mod data_size;
mod database_url;
mod duration;
mod profiles;

use std::time::Duration;

use secrecy::SecretString;

pub use data_size::{InvalidDataSize, parse_data_size};
pub use database_url::{DatabaseUrl, InvalidDatabaseUrl};
pub use duration::{InvalidDuration, parse_duration};
pub use profiles::Profiles;

/// Every limit the API enforces (`ledgerly.limits.*`, Java's `LedgerlyLimits`). All counts are at least 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest page of a list.
    pub max_page_size: u32,
    /// Most items in one batch request, and the chunk size of an import.
    pub max_batch_size: u32,
    /// Largest uploaded file, in bytes.
    pub max_upload_size: u64,
    /// Most rows in one CSV import.
    pub max_import_rows: u32,
    /// Imports per user, team and UTC day.
    pub max_imports_per_day: u32,
    /// Largest JSON request body, in bytes.
    pub max_json_body_size: u64,
    /// Most rows in one CSV export.
    pub max_export_rows: u32,
    /// Requests per minute for one signed-in user.
    pub requests_per_minute: u32,
    /// Requests per minute for one client address on the endpoints that need no token.
    pub auth_requests_per_minute: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_page_size: 100,
            max_batch_size: 100,
            max_upload_size: 10 * 1024 * 1024,
            max_import_rows: 10_000,
            max_imports_per_day: 5,
            max_json_body_size: 1024 * 1024,
            max_export_rows: 10_000,
            requests_per_minute: 600,
            auth_requests_per_minute: 60,
        }
    }
}

/// The two listeners and how long a shutdown may take (`server.port`, `management.server.port`,
/// `spring.lifecycle.timeout-per-shutdown-phase`). Port 0 lets the operating system pick one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerConfig {
    pub api_port: u16,
    pub management_port: u16,
    /// After SIGTERM, how long requests in flight get to finish.
    pub shutdown_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            api_port: 8080,
            management_port: 8081,
            shutdown_timeout: Duration::from_secs(30),
        }
    }
}

/// The Postgres database (`spring.datasource.*`).
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: DatabaseUrl,
    pub username: String,
    pub password: SecretString,
}

/// Sign-in, tokens and lockout (`ledgerly.auth.*`, Java's `AuthProperties`).
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub access_token_ttl: Duration,
    pub refresh_token_ttl: Duration,
    pub verification_ttl: Duration,
    pub max_failed_logins: u32,
    pub lock_duration: Duration,
    /// The ES256 signing key as configured: PKCS#8 PEM or bare Base64. `None` in `local` and `test`, where a
    /// throw-away key is generated, and with `keycloak`, where the app signs nothing.
    pub jwt_private_key: Option<SecretString>,
    /// The matching public key.
    pub jwt_public_key: Option<String>,
}

/// Where tokens come from when Keycloak replaces Ledgerly's own login (profile `keycloak`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeycloakConfig {
    /// The realm's issuer. Tokens from any other issuer are refused.
    pub issuer_uri: String,
    /// Only tokens with this audience are accepted.
    pub audience: String,
}

impl KeycloakConfig {
    /// `application-keycloak.yaml`: the realm that `compose.yaml` starts.
    pub const DEFAULT_ISSUER_URI: &str = "http://localhost:8180/realms/ledgerly";
    pub const DEFAULT_AUDIENCE: &str = "ledgerly-api";
}

/// `ledgerly.teams.*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamsConfig {
    pub invitation_ttl: Duration,
}

impl Default for TeamsConfig {
    fn default() -> Self {
        Self {
            invitation_ttl: Duration::from_secs(7 * 86_400),
        }
    }
}

/// The malware scanner (`ledgerly.receipts.clamav.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClamavConfig {
    pub host: String,
    pub port: u16,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
}

impl ClamavConfig {
    pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
    pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);
}

/// The object store (`ledgerly.storage.s3.*`). Java masks both keys in `toString()`; here both are secrets.
#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: SecretString,
    pub secret_key: SecretString,
}

impl S3Config {
    pub const DEFAULT_REGION: &str = "us-east-1";
}

/// `ledgerly.security.*`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SecurityConfig {
    /// Browser origins that may call the API from JavaScript. Each one passed [`is_valid_origin`].
    pub allowed_origins: Vec<String>,
}

/// True for an exact origin: `http` or `https`, a host, optionally a port, and nothing after it.
///
/// The same rule as Java's `^https?://[A-Za-z0-9.-]+(:[0-9]{1,5})?$`. A wildcard, a path or a trailing slash
/// makes an origin invalid, and an invalid origin stops the application from starting.
#[must_use]
pub fn is_valid_origin(origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
    else {
        return false;
    };
    let (host, port) = match rest.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (rest, None),
    };
    let host_ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let port_ok = port.is_none_or(|p| (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()));
    host_ok && port_ok
}

/// Redelivery of events whose listener never finished (`ledgerly.events.*`, Java's `EventMaintenance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventsConfig {
    pub resubmit_enabled: bool,
    pub resubmit_interval: Duration,
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self {
            resubmit_enabled: true,
            resubmit_interval: Duration::from_secs(60),
        }
    }
}

/// Trace export (`management.opentelemetry.tracing.export.otlp.*`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TracingConfig {
    /// The OTLP/HTTP traces endpoint, such as `http://lgtm:4318/v1/traces`. Without one, nothing is exported.
    pub otlp_endpoint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Java: `LedgerlyApplicationTest` asserts page 100, batch 100, upload 10MB, rows 10000, imports/day 5.
    #[test]
    fn limits_default_to_javas_values() {
        let limits = Limits::default();
        assert_eq!(limits.max_page_size, 100);
        assert_eq!(limits.max_batch_size, 100);
        assert_eq!(limits.max_upload_size, 10_485_760);
        assert_eq!(limits.max_import_rows, 10_000);
        assert_eq!(limits.max_imports_per_day, 5);
        assert_eq!(limits.max_json_body_size, 1_048_576);
        assert_eq!(limits.max_export_rows, 10_000);
        assert_eq!(limits.requests_per_minute, 600);
        assert_eq!(limits.auth_requests_per_minute, 60);
    }

    #[test]
    fn the_other_defaults_are_javas() {
        let server = ServerConfig::default();
        assert_eq!((server.api_port, server.management_port), (8080, 8081));
        assert_eq!(server.shutdown_timeout, Duration::from_secs(30));
        assert_eq!(TeamsConfig::default().invitation_ttl, Duration::from_secs(7 * 86_400));
        assert_eq!(
            EventsConfig::default(),
            EventsConfig {
                resubmit_enabled: true,
                resubmit_interval: Duration::from_secs(60)
            }
        );
        assert_eq!(TracingConfig::default().otlp_endpoint, None);
        assert!(SecurityConfig::default().allowed_origins.is_empty());
    }

    #[test]
    fn exact_origins_are_valid() {
        for origin in [
            "https://app.example.com",
            "http://localhost:5173",
            "https://a-b.example:65535",
        ] {
            assert!(is_valid_origin(origin), "{origin}");
        }
    }

    #[test]
    fn wildcards_paths_and_other_schemes_are_not_origins() {
        for origin in [
            "",
            "*",
            "https://*.example.com",
            "https://app.example.com/",
            "https://app.example.com/path",
            "app.example.com",
            "ftp://app.example.com",
            "https://",
            "https://app.example.com:",
            "https://host:123456",
            "https://host:80a",
            "https://user@host",
            "HTTPS://app.example.com",
            " https://app.example.com",
        ] {
            assert!(!is_valid_origin(origin), "{origin:?}");
        }
    }
}
