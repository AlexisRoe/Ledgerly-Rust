//! Reads the configuration from the environment, under the names Ledgerly-Java reads (ADR 0013).
//!
//! Java counterpart: Spring Boot's property binding, `application*.yaml`, and
//! `shared/config/ProductionEnvironmentCheck.java` with its failure analyzer.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use ledgerly_shared::config::{
    AuthConfig, ClamavConfig, DatabaseConfig, DatabaseUrl, EventsConfig, KeycloakConfig, Limits, Profiles, S3Config,
    SecurityConfig, ServerConfig, TeamsConfig, TracingConfig, is_valid_origin, parse_data_size, parse_duration,
};
use secrecy::SecretString;

/// Everything the service is configured with, read once at startup.
#[derive(Debug, Clone)]
pub struct Config {
    pub profiles: Profiles,
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub limits: Limits,
    pub auth: AuthConfig,
    /// Present exactly when the `keycloak` profile is active.
    pub keycloak: Option<KeycloakConfig>,
    pub teams: TeamsConfig,
    pub clamav: ClamavConfig,
    pub storage: S3Config,
    pub security: SecurityConfig,
    pub events: EventsConfig,
    pub tracing: TracingConfig,
}

/// Where settings come from. The process environment in the binary, a map in tests.
pub trait Environment {
    /// The value of a variable, or `None` if it is not set.
    fn var(&self, name: &str) -> Option<String>;
}

/// The environment of this process. A value that is not valid Unicode counts as not set.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

impl<S: std::hash::BuildHasher> Environment for HashMap<&str, &str, S> {
    fn var(&self, name: &str) -> Option<String> {
        self.get(name).map(|value| (*value).to_owned())
    }
}

/// Why the service cannot start with this environment. Lists every problem at once, so a deployment is fixed
/// in one go. Neither the `Display` nor the `Debug` output ever contains a configured value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct ConfigError {
    prod: bool,
    /// Property names of required settings without a value, in the order Java reports them.
    missing: Vec<&'static str>,
    /// Environment variable and what is wrong with its value.
    invalid: Vec<(String, String)>,
}

impl ConfigError {
    /// The properties that are required and have no value, such as `spring.datasource.url`.
    #[must_use]
    pub fn missing(&self) -> &[&'static str] {
        &self.missing
    }

    /// The environment variables whose value cannot be used.
    pub fn invalid(&self) -> impl Iterator<Item = &str> {
        self.invalid.iter().map(|(variable, _)| variable.as_str())
    }
}

/// The environment variable that sets a property: `ledgerly.storage.s3.bucket` → `LEDGERLY_STORAGE_S3_BUCKET`.
#[must_use]
pub fn environment_variable(property: &str) -> String {
    property.replace(['.', '-'], "_").to_uppercase()
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.missing.is_empty() {
            // In prod, the wording of Java's `ProductionConfigurationFailureAnalyzer`.
            if self.prod {
                f.write_str("The prod profile needs these settings, which are not set:\n")?;
            } else {
                f.write_str("These settings are needed and not set:\n")?;
            }
            for property in &self.missing {
                write!(
                    f,
                    "\n  - {property} (environment variable {})",
                    environment_variable(property)
                )?;
            }
            f.write_str("\n\n")?;
            if self.prod {
                f.write_str(
                    "Set them in the environment of the process (or the container). Nothing in prod has a default: \
                     a made-up database, key or bucket would be worse than not starting.",
                )?;
            } else {
                f.write_str(
                    "Set them in the environment of the process (or the container). The `local` profile has \
                     defaults for the object store and the malware scanner; nothing else has one.",
                )?;
            }
        }
        if !self.invalid.is_empty() {
            if !self.missing.is_empty() {
                f.write_str("\n\n")?;
            }
            f.write_str("These settings have a value that cannot be used:\n")?;
            for (variable, reason) in &self.invalid {
                write!(f, "\n  - {variable}: {reason}")?;
            }
        }
        Ok(())
    }
}

impl Config {
    /// Reads and checks the whole configuration.
    ///
    /// A variable that is empty or only whitespace counts as not set, as in Java's production check.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] listing every required setting that is missing and every value that cannot
    /// be used.
    pub fn from_environment(env: &dyn Environment) -> Result<Self, ConfigError> {
        let profiles = Profiles::parse(&env.var("SPRING_PROFILES_ACTIVE").unwrap_or_default());
        let mut reader = Reader {
            env,
            missing: Vec::new(),
            invalid: Vec::new(),
        };

        // Java's `ProductionEnvironmentCheck.REQUIRED`, in its order. Java checks the list only in prod, but the
        // other profiles cannot start without these either: the properties records are `@NotBlank` everywhere.
        let url = reader
            .required("spring.datasource.url", None)
            .and_then(|url| reader.convert("SPRING_DATASOURCE_URL", &url, DatabaseUrl::parse));
        let username = reader.required("spring.datasource.username", None);
        let password = reader.required_secret("spring.datasource.password", None);

        // `local` and `test` generate a throw-away key; with Keycloak the app signs nothing.
        let own_keys_needed = !(profiles.local || profiles.test || profiles.keycloak);
        let (jwt_private_key, jwt_public_key) = if own_keys_needed {
            (
                reader.required_secret("ledgerly.auth.jwt-private-key", None),
                reader.required("ledgerly.auth.jwt-public-key", None),
            )
        } else {
            (
                reader.secret("LEDGERLY_AUTH_JWT_PRIVATE_KEY"),
                reader.text("LEDGERLY_AUTH_JWT_PUBLIC_KEY"),
            )
        };

        // `application-local.yaml`: where compose.yaml puts the object store and the scanner.
        let local = |default: &'static str| profiles.local.then_some(default);
        let endpoint = reader.required("ledgerly.storage.s3.endpoint", local("http://localhost:8333"));
        let bucket = reader.required("ledgerly.storage.s3.bucket", local("ledgerly-files"));
        let access_key = reader.required_secret("ledgerly.storage.s3.access-key", local("ledgerly"));
        let secret_key = reader.required_secret("ledgerly.storage.s3.secret-key", local("ledgerly-secret"));
        let clamav_host = reader.required("ledgerly.receipts.clamav.host", local("localhost"));
        let clamav_port = reader
            .required("ledgerly.receipts.clamav.port", local("3310"))
            .and_then(|port| reader.convert("LEDGERLY_RECEIPTS_CLAMAV_PORT", &port, parse_port));

        let keycloak = if profiles.keycloak {
            let issuer_uri = reader.issuer_uri();
            let audience = reader
                .text("LEDGERLY_AUTH_KEYCLOAK_AUDIENCE")
                .unwrap_or_else(|| KeycloakConfig::DEFAULT_AUDIENCE.to_owned());
            issuer_uri.map(|issuer_uri| KeycloakConfig { issuer_uri, audience })
        } else {
            None
        };

        let server_defaults = ServerConfig::default();
        let server = ServerConfig {
            api_port: reader.parsed("SERVER_PORT", server_defaults.api_port, parse_listen_port),
            management_port: reader.parsed(
                "MANAGEMENT_SERVER_PORT",
                server_defaults.management_port,
                parse_listen_port,
            ),
            shutdown_timeout: reader.duration(
                "SPRING_LIFECYCLE_TIMEOUT_PER_SHUTDOWN_PHASE",
                server_defaults.shutdown_timeout,
            ),
        };

        let defaults = Limits::default();
        let limits = Limits {
            max_page_size: reader.count("LEDGERLY_LIMITS_MAX_PAGE_SIZE", defaults.max_page_size),
            max_batch_size: reader.count("LEDGERLY_LIMITS_MAX_BATCH_SIZE", defaults.max_batch_size),
            max_upload_size: reader.size("LEDGERLY_LIMITS_MAX_UPLOAD_SIZE", defaults.max_upload_size),
            max_import_rows: reader.count("LEDGERLY_LIMITS_MAX_IMPORT_ROWS", defaults.max_import_rows),
            max_imports_per_day: reader.count("LEDGERLY_LIMITS_MAX_IMPORTS_PER_DAY", defaults.max_imports_per_day),
            max_json_body_size: reader.size("LEDGERLY_LIMITS_MAX_JSON_BODY_SIZE", defaults.max_json_body_size),
            max_export_rows: reader.count("LEDGERLY_LIMITS_MAX_EXPORT_ROWS", defaults.max_export_rows),
            requests_per_minute: reader.count("LEDGERLY_LIMITS_REQUESTS_PER_MINUTE", defaults.requests_per_minute),
            auth_requests_per_minute: reader.count(
                "LEDGERLY_LIMITS_AUTH_REQUESTS_PER_MINUTE",
                defaults.auth_requests_per_minute,
            ),
        };

        let auth = AuthConfig {
            access_token_ttl: reader.duration("LEDGERLY_AUTH_ACCESS_TOKEN_TTL", Duration::from_secs(10 * 60)),
            refresh_token_ttl: reader.duration("LEDGERLY_AUTH_REFRESH_TOKEN_TTL", Duration::from_secs(14 * 86_400)),
            verification_ttl: reader.duration("LEDGERLY_AUTH_VERIFICATION_TTL", Duration::from_secs(86_400)),
            max_failed_logins: reader.count("LEDGERLY_AUTH_MAX_FAILED_LOGINS", 5),
            lock_duration: reader.duration("LEDGERLY_AUTH_LOCK_DURATION", Duration::from_secs(15 * 60)),
            jwt_private_key,
            jwt_public_key,
        };
        let teams = TeamsConfig {
            invitation_ttl: reader.duration("LEDGERLY_TEAMS_INVITATION_TTL", TeamsConfig::default().invitation_ttl),
        };
        let connect_timeout = reader.duration(
            "LEDGERLY_RECEIPTS_CLAMAV_CONNECT_TIMEOUT",
            ClamavConfig::DEFAULT_CONNECT_TIMEOUT,
        );
        let read_timeout = reader.duration(
            "LEDGERLY_RECEIPTS_CLAMAV_READ_TIMEOUT",
            ClamavConfig::DEFAULT_READ_TIMEOUT,
        );
        let region = reader
            .text("LEDGERLY_STORAGE_S3_REGION")
            .unwrap_or_else(|| S3Config::DEFAULT_REGION.to_owned());
        let security = SecurityConfig {
            allowed_origins: reader.allowed_origins(),
        };
        let events_defaults = EventsConfig::default();
        let events = EventsConfig {
            resubmit_enabled: reader.parsed(
                "LEDGERLY_EVENTS_RESUBMIT_ENABLED",
                events_defaults.resubmit_enabled,
                parse_bool,
            ),
            resubmit_interval: reader.duration("LEDGERLY_EVENTS_RESUBMIT_INTERVAL", events_defaults.resubmit_interval),
        };
        let tracing = TracingConfig {
            otlp_endpoint: reader.text("MANAGEMENT_OPENTELEMETRY_TRACING_EXPORT_OTLP_ENDPOINT"),
        };

        // Every `None` below has put an entry into `missing` or `invalid`, so the error is never empty.
        let error = ConfigError {
            prod: profiles.prod,
            missing: reader.missing,
            invalid: reader.invalid,
        };
        if !error.missing.is_empty() || !error.invalid.is_empty() {
            return Err(error);
        }
        match (
            (url, username, password),
            (endpoint, bucket, access_key, secret_key),
            (clamav_host, clamav_port),
            (profiles.keycloak, keycloak),
        ) {
            (
                (Some(url), Some(username), Some(password)),
                (Some(endpoint), Some(bucket), Some(access_key), Some(secret_key)),
                (Some(host), Some(port)),
                (true, keycloak @ Some(_)) | (false, keycloak @ None),
            ) => Ok(Self {
                profiles,
                server,
                database: DatabaseConfig {
                    url,
                    username,
                    password,
                },
                limits,
                auth,
                keycloak,
                teams,
                clamav: ClamavConfig {
                    host,
                    port,
                    connect_timeout,
                    read_timeout,
                },
                storage: S3Config {
                    endpoint,
                    region,
                    bucket,
                    access_key,
                    secret_key,
                },
                security,
                events,
                tracing,
            }),
            _ => Err(error),
        }
    }
}

const ISSUER_PROPERTY: &str = "spring.security.oauth2.resourceserver.jwt.issuer-uri";

/// Reads single settings and collects what is wrong instead of stopping at the first problem.
struct Reader<'a> {
    env: &'a dyn Environment,
    missing: Vec<&'static str>,
    invalid: Vec<(String, String)>,
}

impl Reader<'_> {
    /// The value of a variable, unless it is unset or blank.
    fn text(&self, variable: &str) -> Option<String> {
        self.env.var(variable).filter(|value| !value.trim().is_empty())
    }

    /// Like [`Reader::text`], wrapped the moment it is read.
    fn secret(&self, variable: &str) -> Option<SecretString> {
        self.text(variable).map(SecretString::from)
    }

    fn required(&mut self, property: &'static str, default: Option<&str>) -> Option<String> {
        let value = self
            .text(&environment_variable(property))
            .or_else(|| default.map(str::to_owned));
        if value.is_none() {
            self.missing.push(property);
        }
        value
    }

    fn required_secret(&mut self, property: &'static str, default: Option<&str>) -> Option<SecretString> {
        self.required(property, default).map(SecretString::from)
    }

    /// Converts a value that is present. The reason for a failure is recorded, the value is not.
    fn convert<T, E: fmt::Display>(
        &mut self,
        variable: &str,
        value: &str,
        parse: impl FnOnce(&str) -> Result<T, E>,
    ) -> Option<T> {
        match parse(value) {
            Ok(parsed) => Some(parsed),
            Err(reason) => {
                self.invalid.push((variable.to_owned(), reason.to_string()));
                None
            }
        }
    }

    /// An optional setting: the default if unset. An unusable value is recorded and the default returned, so
    /// that reading can go on; the caller fails afterwards because `invalid` is not empty.
    fn parsed<T, E: fmt::Display>(
        &mut self,
        variable: &str,
        default: T,
        parse: impl FnOnce(&str) -> Result<T, E>,
    ) -> T {
        match self.text(variable) {
            None => default,
            Some(value) => self.convert(variable, &value, parse).unwrap_or(default),
        }
    }

    fn count(&mut self, variable: &str, default: u32) -> u32 {
        self.parsed(variable, default, parse_count)
    }

    fn duration(&mut self, variable: &str, default: Duration) -> Duration {
        self.parsed(variable, default, parse_duration)
    }

    fn size(&mut self, variable: &str, default: u64) -> u64 {
        self.parsed(variable, default, |text| match parse_data_size(text) {
            Ok(0) => Err("must be greater than 0".to_owned()),
            Ok(bytes) => Ok(bytes),
            Err(error) => Err(error.to_string()),
        })
    }

    /// The issuer as `application-keycloak.yaml` resolves it: the property itself, else `KEYCLOAK_ISSUER_URI`,
    /// else the local realm. Only a variable that is set but blank leaves it missing, exactly as in Java,
    /// where the YAML default applies in prod too.
    fn issuer_uri(&mut self) -> Option<String> {
        let configured = self
            .env
            .var(&environment_variable(ISSUER_PROPERTY))
            .or_else(|| self.env.var("KEYCLOAK_ISSUER_URI"));
        match configured {
            None => Some(KeycloakConfig::DEFAULT_ISSUER_URI.to_owned()),
            Some(value) if value.trim().is_empty() => {
                self.missing.push(ISSUER_PROPERTY);
                None
            }
            Some(value) => Some(value),
        }
    }

    /// `LEDGERLY_SECURITY_ALLOWED_ORIGINS` as a comma-separated list, or Spring's indexed form
    /// `LEDGERLY_SECURITY_ALLOWED_ORIGINS_0`, `_1`, … Entries are trimmed; each must be an exact origin.
    fn allowed_origins(&mut self) -> Vec<String> {
        const VARIABLE: &str = "LEDGERLY_SECURITY_ALLOWED_ORIGINS";
        let mut found: Vec<(String, String)> = Vec::new();
        if let Some(list) = self.text(VARIABLE) {
            found.extend(
                list.split(',')
                    .map(str::trim)
                    .filter(|origin| !origin.is_empty())
                    .map(|origin| (VARIABLE.to_owned(), origin.to_owned())),
            );
        } else {
            for index in 0_u32.. {
                let variable = format!("{VARIABLE}_{index}");
                match self.text(&variable) {
                    Some(origin) => found.push((variable, origin.trim().to_owned())),
                    None => break,
                }
            }
        }
        let mut origins = Vec::new();
        for (variable, origin) in found {
            if is_valid_origin(&origin) {
                origins.push(origin);
            } else if !self.invalid.iter().any(|(known, _)| *known == variable) {
                self.invalid.push((
                    variable,
                    "every entry must be an exact origin such as https://app.example.com: \
                     no wildcard, no path, no trailing slash"
                        .to_owned(),
                ));
            }
        }
        origins
    }
}

/// A count that must be at least 1 and fit Java's `int` (`@Positive int`).
fn parse_count(text: &str) -> Result<u32, &'static str> {
    const MESSAGE: &str = "must be a whole number from 1 to 2147483647";
    match u32::from_str(text.trim()) {
        Ok(count) if count >= 1 && i32::try_from(count).is_ok() => Ok(count),
        _ => Err(MESSAGE),
    }
}

/// A port to connect to: 1 to 65535.
fn parse_port(text: &str) -> Result<u16, &'static str> {
    match u16::from_str(text.trim()) {
        Ok(port) if port != 0 => Ok(port),
        _ => Err("must be a port number from 1 to 65535"),
    }
}

/// A port to listen on: 0 lets the operating system pick one.
fn parse_listen_port(text: &str) -> Result<u16, &'static str> {
    u16::from_str(text.trim()).map_err(|_| "must be a port number from 0 to 65535")
}

/// The spellings Spring's `StringToBooleanConverter` accepts.
fn parse_bool(text: &str) -> Result<bool, &'static str> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Ok(true),
        "false" | "off" | "no" | "0" => Ok(false),
        _ => Err("must be true or false"),
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    type Env = HashMap<&'static str, &'static str>;

    /// Java: `ProductionEnvironmentCheckTest.EVERYTHING`.
    fn everything() -> Env {
        HashMap::from([
            ("SPRING_PROFILES_ACTIVE", "prod"),
            ("SPRING_DATASOURCE_URL", "jdbc:postgresql://db:5432/ledgerly"),
            ("SPRING_DATASOURCE_USERNAME", "ledgerly"),
            ("SPRING_DATASOURCE_PASSWORD", "pw-4f9a"),
            ("LEDGERLY_AUTH_JWT_PRIVATE_KEY", "private-7c1d"),
            ("LEDGERLY_AUTH_JWT_PUBLIC_KEY", "public"),
            ("LEDGERLY_STORAGE_S3_ENDPOINT", "https://s3.example.com"),
            ("LEDGERLY_STORAGE_S3_BUCKET", "files"),
            ("LEDGERLY_STORAGE_S3_ACCESS_KEY", "key-2b8e"),
            ("LEDGERLY_STORAGE_S3_SECRET_KEY", "secret-9d3f"),
            ("LEDGERLY_RECEIPTS_CLAMAV_HOST", "clamav"),
            ("LEDGERLY_RECEIPTS_CLAMAV_PORT", "3310"),
        ])
    }

    fn local() -> Env {
        HashMap::from([
            ("SPRING_PROFILES_ACTIVE", "local"),
            ("SPRING_DATASOURCE_URL", "jdbc:postgresql://localhost:5432/ledgerly"),
            ("SPRING_DATASOURCE_USERNAME", "ledgerly"),
            ("SPRING_DATASOURCE_PASSWORD", "ledgerly"),
        ])
    }

    fn with(mut env: Env, changes: &[(&'static str, &'static str)]) -> Env {
        env.extend(changes.iter().copied());
        env
    }

    fn load(env: &Env) -> Result<Config, ConfigError> {
        Config::from_environment(env)
    }

    const REQUIRED: [&str; 11] = [
        "spring.datasource.url",
        "spring.datasource.username",
        "spring.datasource.password",
        "ledgerly.auth.jwt-private-key",
        "ledgerly.auth.jwt-public-key",
        "ledgerly.storage.s3.endpoint",
        "ledgerly.storage.s3.bucket",
        "ledgerly.storage.s3.access-key",
        "ledgerly.storage.s3.secret-key",
        "ledgerly.receipts.clamav.host",
        "ledgerly.receipts.clamav.port",
    ];

    // --- Java: ProductionEnvironmentCheckTest ---

    #[test]
    fn prod_with_nothing_set_reports_every_missing_setting_at_once() {
        let error = load(&HashMap::from([("SPRING_PROFILES_ACTIVE", "prod")])).unwrap_err();

        assert_eq!(error.missing(), REQUIRED);
        let message = error.to_string();
        for line in [
            "spring.datasource.url (environment variable SPRING_DATASOURCE_URL)",
            "ledgerly.auth.jwt-private-key (environment variable LEDGERLY_AUTH_JWT_PRIVATE_KEY)",
            "ledgerly.storage.s3.secret-key (environment variable LEDGERLY_STORAGE_S3_SECRET_KEY)",
            "ledgerly.receipts.clamav.host",
        ] {
            assert!(message.contains(line), "{message}");
        }
    }

    #[test]
    fn blank_values_count_as_missing() {
        let env = with(
            everything(),
            &[
                ("LEDGERLY_STORAGE_S3_SECRET_KEY", "   "),
                ("SPRING_DATASOURCE_PASSWORD", ""),
            ],
        );

        assert_eq!(
            load(&env).unwrap_err().missing(),
            ["spring.datasource.password", "ledgerly.storage.s3.secret-key"]
        );
    }

    #[test]
    fn prod_with_everything_set_starts() {
        let config = load(&everything()).unwrap();

        assert!(config.profiles.prod);
        assert_eq!(config.database.url.host(), "db");
        assert_eq!(config.database.username, "ledgerly");
        assert_eq!(config.database.password.expose_secret(), "pw-4f9a");
        assert_eq!(config.storage.endpoint, "https://s3.example.com");
        assert_eq!(config.storage.bucket, "files");
        assert_eq!(config.clamav.host, "clamav");
        assert_eq!(config.clamav.port, 3310);
        assert_eq!(config.auth.jwt_public_key.as_deref(), Some("public"));
        assert_eq!(config.keycloak, None);
    }

    #[test]
    fn with_keycloak_no_signing_keys_are_needed() {
        let mut env = with(everything(), &[("SPRING_PROFILES_ACTIVE", "prod,keycloak")]);
        env.remove("LEDGERLY_AUTH_JWT_PRIVATE_KEY");
        env.remove("LEDGERLY_AUTH_JWT_PUBLIC_KEY");
        let env = with(
            env,
            &[(
                "SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI",
                "https://sso.example.com/realms/ledgerly",
            )],
        );

        let config = load(&env).unwrap();

        assert!(config.auth.jwt_private_key.is_none());
        assert_eq!(
            config.keycloak,
            Some(KeycloakConfig {
                issuer_uri: "https://sso.example.com/realms/ledgerly".to_owned(),
                audience: "ledgerly-api".to_owned(),
            })
        );
    }

    #[test]
    fn with_keycloak_a_blank_issuer_is_missing() {
        let env = with(
            everything(),
            &[
                ("SPRING_PROFILES_ACTIVE", "prod,keycloak"),
                ("SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI", " "),
            ],
        );

        let error = load(&env).unwrap_err();

        assert_eq!(
            error.missing(),
            ["spring.security.oauth2.resourceserver.jwt.issuer-uri"]
        );
        assert!(error.to_string().contains(
            "spring.security.oauth2.resourceserver.jwt.issuer-uri \
             (environment variable SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI)"
        ));
    }

    /// `application-keycloak.yaml` gives the issuer a default (`${KEYCLOAK_ISSUER_URI:http://localhost:8180/…}`)
    /// that also applies in prod, so Java starts `prod,keycloak` without an issuer. Open question in the
    /// learning log (M1).
    #[test]
    fn with_keycloak_an_unset_issuer_falls_back_to_the_yaml_default_even_in_prod() {
        let env = with(everything(), &[("SPRING_PROFILES_ACTIVE", "prod,keycloak")]);
        assert_eq!(
            load(&env).unwrap().keycloak.unwrap().issuer_uri,
            "http://localhost:8180/realms/ledgerly"
        );

        let env = with(env, &[("KEYCLOAK_ISSUER_URI", "https://id.example.com/realms/x")]);
        assert_eq!(
            load(&env).unwrap().keycloak.unwrap().issuer_uri,
            "https://id.example.com/realms/x"
        );
    }

    #[test]
    fn the_failure_reads_as_an_actionable_message() {
        let mut env = everything();
        env.remove("SPRING_DATASOURCE_URL");

        assert_eq!(
            load(&env).unwrap_err().to_string(),
            "The prod profile needs these settings, which are not set:\n\
             \n  - spring.datasource.url (environment variable SPRING_DATASOURCE_URL)\n\
             \n\
             Set them in the environment of the process (or the container). Nothing in prod has a default: a \
             made-up database, key or bucket would be worse than not starting."
        );
    }

    // --- profiles ---

    #[test]
    fn local_has_defaults_for_the_object_store_and_the_scanner_and_needs_no_keys() {
        let config = load(&local()).unwrap();

        assert!(config.profiles.local);
        assert_eq!(config.storage.endpoint, "http://localhost:8333");
        assert_eq!(config.storage.bucket, "ledgerly-files");
        assert_eq!(config.storage.access_key.expose_secret(), "ledgerly");
        assert_eq!(config.storage.secret_key.expose_secret(), "ledgerly-secret");
        assert_eq!(config.storage.region, "us-east-1");
        assert_eq!((config.clamav.host.as_str(), config.clamav.port), ("localhost", 3310));
        assert!(config.auth.jwt_private_key.is_none() && config.auth.jwt_public_key.is_none());
    }

    #[test]
    fn local_defaults_can_be_overridden() {
        let env = with(
            local(),
            &[
                ("LEDGERLY_STORAGE_S3_ENDPOINT", "http://seaweedfs:8333"),
                ("LEDGERLY_RECEIPTS_CLAMAV_HOST", "clamav"),
            ],
        );
        let config = load(&env).unwrap();

        assert_eq!(config.storage.endpoint, "http://seaweedfs:8333");
        assert_eq!(config.clamav.host, "clamav");
    }

    #[test]
    fn local_still_needs_the_database() {
        let error = load(&HashMap::from([("SPRING_PROFILES_ACTIVE", "local")])).unwrap_err();

        assert_eq!(
            error.missing(),
            [
                "spring.datasource.url",
                "spring.datasource.username",
                "spring.datasource.password"
            ]
        );
        assert!(
            error
                .to_string()
                .starts_with("These settings are needed and not set:\n")
        );
    }

    #[test]
    fn test_profile_needs_no_keys_but_has_no_local_defaults() {
        let error = load(&with(local(), &[("SPRING_PROFILES_ACTIVE", "test")])).unwrap_err();

        assert_eq!(error.missing(), &REQUIRED[5..]);
    }

    #[test]
    fn without_any_profile_everything_is_required() {
        assert_eq!(load(&HashMap::new()).unwrap_err().missing(), REQUIRED);
    }

    #[test]
    fn local_with_keycloak_points_at_the_compose_realm() {
        let config = load(&with(local(), &[("SPRING_PROFILES_ACTIVE", "local,keycloak")])).unwrap();

        assert_eq!(
            config.keycloak,
            Some(KeycloakConfig {
                issuer_uri: "http://localhost:8180/realms/ledgerly".to_owned(),
                audience: "ledgerly-api".to_owned(),
            })
        );
    }

    // --- defaults and overrides ---

    #[test]
    fn everything_optional_has_javas_default() {
        let config = load(&local()).unwrap();

        assert_eq!(config.limits, Limits::default());
        assert_eq!(config.server, ServerConfig::default());
        assert_eq!(config.auth.access_token_ttl, Duration::from_secs(600));
        assert_eq!(config.auth.refresh_token_ttl, Duration::from_secs(14 * 86_400));
        assert_eq!(config.auth.verification_ttl, Duration::from_secs(86_400));
        assert_eq!(config.auth.max_failed_logins, 5);
        assert_eq!(config.auth.lock_duration, Duration::from_secs(900));
        assert_eq!(config.teams.invitation_ttl, Duration::from_secs(7 * 86_400));
        assert_eq!(config.clamav.connect_timeout, Duration::from_secs(2));
        assert_eq!(config.clamav.read_timeout, Duration::from_secs(30));
        assert!(config.security.allowed_origins.is_empty());
        assert_eq!(config.events, EventsConfig::default());
        assert_eq!(config.tracing.otlp_endpoint, None);
    }

    #[test]
    fn every_optional_setting_can_be_set() {
        let env = with(
            local(),
            &[
                ("SERVER_PORT", "9000"),
                ("MANAGEMENT_SERVER_PORT", "0"),
                ("SPRING_LIFECYCLE_TIMEOUT_PER_SHUTDOWN_PHASE", "5s"),
                ("LEDGERLY_LIMITS_MAX_PAGE_SIZE", "50"),
                ("LEDGERLY_LIMITS_MAX_BATCH_SIZE", "20"),
                ("LEDGERLY_LIMITS_MAX_UPLOAD_SIZE", "2MB"),
                ("LEDGERLY_LIMITS_MAX_IMPORT_ROWS", "500"),
                ("LEDGERLY_LIMITS_MAX_IMPORTS_PER_DAY", "2"),
                ("LEDGERLY_LIMITS_MAX_JSON_BODY_SIZE", "4096"),
                ("LEDGERLY_LIMITS_MAX_EXPORT_ROWS", "1200"),
                ("LEDGERLY_LIMITS_REQUESTS_PER_MINUTE", "250"),
                ("LEDGERLY_LIMITS_AUTH_REQUESTS_PER_MINUTE", "12"),
                ("LEDGERLY_AUTH_ACCESS_TOKEN_TTL", "5m"),
                ("LEDGERLY_AUTH_REFRESH_TOKEN_TTL", "P1D"),
                ("LEDGERLY_AUTH_VERIFICATION_TTL", "1h"),
                ("LEDGERLY_AUTH_MAX_FAILED_LOGINS", "3"),
                ("LEDGERLY_AUTH_LOCK_DURATION", "PT30S"),
                ("LEDGERLY_AUTH_KEYCLOAK_AUDIENCE", "ignored-without-the-profile"),
                ("LEDGERLY_TEAMS_INVITATION_TTL", "2d"),
                ("LEDGERLY_RECEIPTS_CLAMAV_CONNECT_TIMEOUT", "500ms"),
                ("LEDGERLY_RECEIPTS_CLAMAV_READ_TIMEOUT", "10s"),
                ("LEDGERLY_STORAGE_S3_REGION", "eu-central-1"),
                (
                    "LEDGERLY_SECURITY_ALLOWED_ORIGINS",
                    "https://app.example.com, http://localhost:5173",
                ),
                ("LEDGERLY_EVENTS_RESUBMIT_ENABLED", "false"),
                ("LEDGERLY_EVENTS_RESUBMIT_INTERVAL", "PT5M"),
                (
                    "MANAGEMENT_OPENTELEMETRY_TRACING_EXPORT_OTLP_ENDPOINT",
                    "http://lgtm:4318/v1/traces",
                ),
            ],
        );
        let config = load(&env).unwrap();

        assert_eq!(
            config.server,
            ServerConfig {
                api_port: 9000,
                management_port: 0,
                shutdown_timeout: Duration::from_secs(5)
            }
        );
        assert_eq!(
            config.limits,
            Limits {
                max_page_size: 50,
                max_batch_size: 20,
                max_upload_size: 2 * 1024 * 1024,
                max_import_rows: 500,
                max_imports_per_day: 2,
                max_json_body_size: 4096,
                max_export_rows: 1200,
                requests_per_minute: 250,
                auth_requests_per_minute: 12,
            }
        );
        assert_eq!(config.auth.access_token_ttl, Duration::from_secs(300));
        assert_eq!(config.auth.refresh_token_ttl, Duration::from_secs(86_400));
        assert_eq!(config.auth.verification_ttl, Duration::from_secs(3600));
        assert_eq!(config.auth.max_failed_logins, 3);
        assert_eq!(config.auth.lock_duration, Duration::from_secs(30));
        assert_eq!(config.keycloak, None);
        assert_eq!(config.teams.invitation_ttl, Duration::from_secs(2 * 86_400));
        assert_eq!(config.clamav.connect_timeout, Duration::from_millis(500));
        assert_eq!(config.clamav.read_timeout, Duration::from_secs(10));
        assert_eq!(config.storage.region, "eu-central-1");
        assert_eq!(
            config.security.allowed_origins,
            ["https://app.example.com", "http://localhost:5173"]
        );
        assert_eq!(
            config.events,
            EventsConfig {
                resubmit_enabled: false,
                resubmit_interval: Duration::from_secs(300)
            }
        );
        assert_eq!(
            config.tracing.otlp_endpoint.as_deref(),
            Some("http://lgtm:4318/v1/traces")
        );
    }

    #[test]
    fn allowed_origins_can_use_springs_indexed_form() {
        let env = with(
            local(),
            &[
                ("LEDGERLY_SECURITY_ALLOWED_ORIGINS_0", "https://a.example.com"),
                ("LEDGERLY_SECURITY_ALLOWED_ORIGINS_1", "https://b.example.com"),
            ],
        );

        assert_eq!(
            load(&env).unwrap().security.allowed_origins,
            ["https://a.example.com", "https://b.example.com"]
        );
    }

    #[test]
    fn the_plain_postgres_url_form_is_accepted() {
        let env = with(local(), &[("SPRING_DATASOURCE_URL", "postgres://db.internal/app")]);
        let config = load(&env).unwrap();

        assert_eq!(
            (config.database.url.host(), config.database.url.port()),
            ("db.internal", 5432)
        );
    }

    // --- unusable values ---

    #[test]
    fn every_unusable_value_is_reported_at_once_by_variable() {
        let env = with(
            local(),
            &[
                ("SPRING_DATASOURCE_URL", "jdbc:mysql://db/ledgerly"),
                ("LEDGERLY_RECEIPTS_CLAMAV_PORT", "0"),
                ("SERVER_PORT", "70000"),
                ("LEDGERLY_LIMITS_MAX_PAGE_SIZE", "0"),
                ("LEDGERLY_LIMITS_MAX_BATCH_SIZE", "-1"),
                ("LEDGERLY_LIMITS_MAX_IMPORT_ROWS", "2147483648"),
                ("LEDGERLY_LIMITS_MAX_UPLOAD_SIZE", "ten"),
                ("LEDGERLY_LIMITS_MAX_JSON_BODY_SIZE", "0"),
                ("LEDGERLY_AUTH_ACCESS_TOKEN_TTL", "soon"),
                ("LEDGERLY_EVENTS_RESUBMIT_ENABLED", "maybe"),
                (
                    "LEDGERLY_SECURITY_ALLOWED_ORIGINS",
                    "https://ok.example.com,https://*.example.com,https://x.example.com/",
                ),
            ],
        );
        let error = load(&env).unwrap_err();

        assert!(error.missing().is_empty());
        let mut invalid: Vec<&str> = error.invalid().collect();
        invalid.sort_unstable();
        assert_eq!(
            invalid,
            [
                "LEDGERLY_AUTH_ACCESS_TOKEN_TTL",
                "LEDGERLY_EVENTS_RESUBMIT_ENABLED",
                "LEDGERLY_LIMITS_MAX_BATCH_SIZE",
                "LEDGERLY_LIMITS_MAX_IMPORT_ROWS",
                "LEDGERLY_LIMITS_MAX_JSON_BODY_SIZE",
                "LEDGERLY_LIMITS_MAX_PAGE_SIZE",
                "LEDGERLY_LIMITS_MAX_UPLOAD_SIZE",
                "LEDGERLY_RECEIPTS_CLAMAV_PORT",
                "LEDGERLY_SECURITY_ALLOWED_ORIGINS",
                "SERVER_PORT",
                "SPRING_DATASOURCE_URL",
            ]
        );
        let message = error.to_string();
        assert!(
            message.starts_with("These settings have a value that cannot be used:\n"),
            "{message}"
        );
        assert!(
            message.contains("\n  - LEDGERLY_LIMITS_MAX_PAGE_SIZE: must be a whole number from 1 to 2147483647"),
            "{message}"
        );
    }

    #[test]
    fn missing_and_unusable_settings_are_reported_together() {
        let mut env = with(everything(), &[("LEDGERLY_RECEIPTS_CLAMAV_PORT", "clam")]);
        env.remove("SPRING_DATASOURCE_USERNAME");
        let error = load(&env).unwrap_err();

        assert_eq!(error.missing(), ["spring.datasource.username"]);
        assert_eq!(error.invalid().collect::<Vec<_>>(), ["LEDGERLY_RECEIPTS_CLAMAV_PORT"]);
        let message = error.to_string();
        assert!(message.contains("which are not set:") && message.contains("cannot be used:"));
    }

    // --- secrets ---

    #[test]
    fn debug_of_the_config_shows_no_secret() {
        let debug = format!("{:?}", load(&everything()).unwrap());

        for secret in ["pw-4f9a", "private-7c1d", "key-2b8e", "secret-9d3f"] {
            assert!(!debug.contains(secret), "{secret} appears in {debug}");
        }
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn an_error_shows_no_configured_value() {
        let env = with(
            everything(),
            &[
                ("SPRING_DATASOURCE_URL", "postgres://admin:hunter2@db/ledgerly"),
                ("LEDGERLY_RECEIPTS_CLAMAV_PORT", "port-4411"),
                ("LEDGERLY_SECURITY_ALLOWED_ORIGINS", "https://evil.example.com/path"),
            ],
        );
        let error = load(&env).unwrap_err();

        for output in [error.to_string(), format!("{error:?}")] {
            for value in [
                "hunter2",
                "admin",
                "port-4411",
                "evil.example.com",
                "pw-4f9a",
                "secret-9d3f",
            ] {
                assert!(!output.contains(value), "{value} appears in {output}");
            }
        }
    }

    #[test]
    fn property_names_map_to_environment_variables_like_springs() {
        assert_eq!(
            environment_variable("ledgerly.storage.s3.bucket"),
            "LEDGERLY_STORAGE_S3_BUCKET"
        );
        assert_eq!(
            environment_variable("ledgerly.auth.jwt-private-key"),
            "LEDGERLY_AUTH_JWT_PRIVATE_KEY"
        );
    }
}
