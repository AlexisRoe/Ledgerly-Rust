//! Run groups: sets of test files that need the app started with a particular configuration.
//!
//! Java gives each test class its own Spring context with property overrides (`@SpringBootTest(properties =
//! …)`, `application-test.yaml`). From outside, configuration can only be set when the process starts, so
//! `just parity` starts the app once per group.

/// One start of the app and the test files that run against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub name: &'static str,
    /// The files under `tests/` (without `.rs`) that belong to this group. The default group has none listed:
    /// it takes every file no other group claims.
    pub files: &'static [&'static str],
    /// Environment of the app, on top of [`BASE_ENVIRONMENT`].
    pub environment: &'static [(&'static str, &'static str)],
    /// Compose services that are stopped while the group runs and started again afterwards. Java flips a
    /// switch on a fake scanner; from outside, an outage is a stopped container (ADR 0011).
    pub stopped_services: &'static [&'static str],
}

/// What every group starts the app with: Java's `test` profile and `src/test/resources/application-test.yaml`,
/// pointed at the services of `compose.yaml` by their names inside the Compose network.
pub const BASE_ENVIRONMENT: &[(&str, &str)] = &[
    ("SPRING_PROFILES_ACTIVE", "test"),
    ("SPRING_DATASOURCE_URL", "jdbc:postgresql://postgres:5432/ledgerly"),
    ("SPRING_DATASOURCE_USERNAME", "ledgerly"),
    ("SPRING_DATASOURCE_PASSWORD", "ledgerly"),
    ("LEDGERLY_STORAGE_S3_ENDPOINT", "http://seaweedfs:8333"),
    ("LEDGERLY_STORAGE_S3_BUCKET", "ledgerly-files"),
    ("LEDGERLY_STORAGE_S3_ACCESS_KEY", "ledgerly"),
    ("LEDGERLY_STORAGE_S3_SECRET_KEY", "ledgerly-secret"),
    ("LEDGERLY_RECEIPTS_CLAMAV_HOST", "clamav"),
    ("LEDGERLY_RECEIPTS_CLAMAV_PORT", "3310"),
    // application-test.yaml: limits high enough that no functional test trips over them.
    ("LEDGERLY_LIMITS_REQUESTS_PER_MINUTE", "100000"),
    ("LEDGERLY_LIMITS_AUTH_REQUESTS_PER_MINUTE", "100000"),
];

/// The group for every file no other group lists.
pub const DEFAULT: Group = Group {
    name: "default",
    files: &[],
    environment: &[],
    stopped_services: &[],
};

/// Every group, the default one first.
pub const GROUPS: &[Group] = &[
    DEFAULT,
    // `OwaspTestBase`: a rate limit a test can reach, and one allowed browser origin.
    Group {
        name: "owasp",
        files: &[
            "owasp_api1_object_level_authorization",
            "owasp_api2_authentication",
            "owasp_api3_object_property_level_authorization",
            "owasp_api5_function_level_authorization",
            "owasp_api6_sensitive_business_flows",
            "owasp_api8_security_misconfiguration",
            "owasp_api10_unsafe_consumption",
            "owasp_log_hygiene",
        ],
        environment: &[
            ("LEDGERLY_LIMITS_REQUESTS_PER_MINUTE", "250"),
            ("LEDGERLY_SECURITY_ALLOWED_ORIGINS", "https://app.example.com"),
        ],
        stopped_services: &[],
    },
    // `Api4UnrestrictedResourceConsumptionTest`: limits low enough to hit in one test.
    Group {
        name: "api4",
        files: &["owasp_api4_resource_consumption"],
        environment: &[
            ("LEDGERLY_LIMITS_REQUESTS_PER_MINUTE", "30"),
            ("LEDGERLY_LIMITS_AUTH_REQUESTS_PER_MINUTE", "12"),
            ("LEDGERLY_SECURITY_ALLOWED_ORIGINS", "https://app.example.com"),
            // Each test claims its own client address in X-Forwarded-For (see `World::client_address`).
            ("SERVER_FORWARD_HEADERS_STRATEGY", "native"),
        ],
        stopped_services: &[],
    },
    // `BatchAndExportApiTest`: an export cap a test can exceed.
    Group {
        name: "export",
        files: &["expenses_batch_and_export"],
        environment: &[("LEDGERLY_LIMITS_MAX_EXPORT_ROWS", "1200")],
        stopped_services: &[],
    },
    // `MetricsTest` compares counters before and after. Its file has the app to itself and runs one test at a
    // time (.config/nextest.toml), so that nothing else moves the counters.
    Group {
        name: "metrics",
        files: &["owasp_metrics"],
        environment: &[("LEDGERLY_LIMITS_REQUESTS_PER_MINUTE", "250")],
        stopped_services: &[],
    },
    // `FakeMalwareScanner.unavailable = true`: the scanner gives no answer.
    Group {
        name: "scanner-down",
        files: &["receipts_scanner_outage"],
        environment: &[],
        stopped_services: &["clamav"],
    },
    // `KeycloakEndToEndTest`: tokens come from the realm in `compose.yaml`. M11 has to settle the issuer: the
    // app reaches Keycloak as `keycloak:8080`, the tests as `127.0.0.1:8180`, and both must see one `iss`.
    Group {
        name: "keycloak",
        files: &["keycloak"],
        environment: &[
            ("SPRING_PROFILES_ACTIVE", "test,keycloak"),
            (
                "SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI",
                "http://keycloak:8080/realms/ledgerly",
            ),
        ],
        stopped_services: &[],
    },
    // `ExternalIdentityTest`: the identity provider cannot be reached.
    Group {
        name: "keycloak-down",
        files: &["keycloak_unreachable"],
        environment: &[
            ("SPRING_PROFILES_ACTIVE", "test,keycloak"),
            (
                "SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI",
                "http://keycloak:8080/realms/ledgerly",
            ),
        ],
        stopped_services: &["keycloak"],
    },
    // `SwaggerUiProfileTest`, `Api9ImproperInventoryManagementTest`: the `local` profile serves the docs.
    Group {
        name: "local",
        files: &["local_profile", "owasp_api9_inventory"],
        environment: &[("SPRING_PROFILES_ACTIVE", "local,test")],
        stopped_services: &[],
    },
];

/// The group a test file runs in.
#[must_use]
pub fn group_of(file: &str) -> &'static Group {
    GROUPS
        .iter()
        .find(|group| group.files.contains(&file))
        .unwrap_or(&DEFAULT)
}

/// The environment the app is started with for `group`: the base, with the group's entries replacing equal
/// names.
#[must_use]
pub fn environment_of(group: &Group) -> Vec<(&'static str, &'static str)> {
    let mut environment: Vec<(&str, &str)> = BASE_ENVIRONMENT
        .iter()
        .filter(|(name, _)| !group.environment.iter().any(|(overridden, _)| overridden == name))
        .copied()
        .collect();
    environment.extend_from_slice(group.environment);
    environment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_no_group_lists_runs_in_the_default_group() {
        assert_eq!(group_of("operational").name, "default");
        assert_eq!(group_of("owasp_api4_resource_consumption").name, "api4");
        assert_eq!(group_of("expenses_batch_and_export").name, "export");
    }

    #[test]
    fn no_file_is_in_two_groups_and_names_are_unique() {
        let mut files: Vec<&str> = GROUPS.iter().flat_map(|group| group.files.iter().copied()).collect();
        let total = files.len();
        files.sort_unstable();
        files.dedup();
        assert_eq!(files.len(), total);

        let mut names: Vec<&str> = GROUPS.iter().map(|group| group.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), GROUPS.len());
    }

    #[test]
    fn a_groups_environment_replaces_the_base_value() {
        let api4 = environment_of(group_of("owasp_api4_resource_consumption"));

        let values = |name: &str| -> Vec<&str> {
            api4.iter()
                .filter(|(key, _)| *key == name)
                .map(|(_, value)| *value)
                .collect()
        };
        assert_eq!(values("LEDGERLY_LIMITS_REQUESTS_PER_MINUTE"), ["30"]);
        assert_eq!(values("LEDGERLY_LIMITS_AUTH_REQUESTS_PER_MINUTE"), ["12"]);
        assert_eq!(values("SPRING_PROFILES_ACTIVE"), ["test"]);
        assert_eq!(environment_of(&DEFAULT), BASE_ENVIRONMENT);
    }

    /// Java's overrides, from the inventory §0 (5).
    #[test]
    fn the_groups_carry_javas_overrides() {
        let value = |file: &str, name: &str| {
            environment_of(group_of(file))
                .into_iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value)
        };
        assert_eq!(
            value(
                "owasp_api1_object_level_authorization",
                "LEDGERLY_LIMITS_REQUESTS_PER_MINUTE"
            ),
            Some("250")
        );
        assert_eq!(
            value(
                "owasp_api8_security_misconfiguration",
                "LEDGERLY_SECURITY_ALLOWED_ORIGINS"
            ),
            Some("https://app.example.com")
        );
        assert_eq!(
            value("expenses_batch_and_export", "LEDGERLY_LIMITS_MAX_EXPORT_ROWS"),
            Some("1200")
        );
        assert_eq!(
            value("owasp_api9_inventory", "SPRING_PROFILES_ACTIVE"),
            Some("local,test")
        );
        assert_eq!(
            value("operational", "LEDGERLY_LIMITS_REQUESTS_PER_MINUTE"),
            Some("100000")
        );
    }
}
