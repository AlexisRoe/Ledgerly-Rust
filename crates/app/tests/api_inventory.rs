//! OWASP API9:2023 Improper Inventory Management: the route table the API router is built from is the spec.
//!
//! Java: `Api9ImproperInventoryManagementTest.everyServedEndpointIsInTheSpecAndEveryDocumentedOneIsServed` and
//! `theComparisonDetectsDriftInBothDirections`, which compare Spring's handler mappings with the spec. Here the
//! table is [`ledgerly_app::routes`]: `router` serves exactly that table, so a route cannot be served without
//! appearing here.

// clippy.toml exempts `#[test]` functions only; the helpers of an integration test are test code too.
#![allow(clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::{BTreeSet, HashMap};

use ledgerly_app::Modules;
use ledgerly_app::config::Config;
use ledgerly_shared::config::Limits;
use serde_json::Value;

const SPEC: &str = include_str!("../../../api/openapi.yaml");
const METHODS: [&str; 7] = ["get", "put", "post", "delete", "options", "head", "patch"];

/// Documented operations that are not served yet, with the milestone that serves them. The test fails when one
/// of them starts being served without being taken off this list, so the list only ever shrinks.
const NOT_YET: &[(&str, &str, &str)] = &[
    ("M6", "GET", "/api/v1/me"),
    ("M6", "POST", "/api/v1/teams"),
    ("M6", "POST", "/api/v1/teams/{teamId}:leave"),
    ("M6", "POST", "/api/v1/teams/{teamId}/invitations"),
    ("M6", "DELETE", "/api/v1/teams/{teamId}/invitations/{invitationId}"),
    ("M6", "PATCH", "/api/v1/teams/{teamId}/members/{userId}"),
    ("M6", "DELETE", "/api/v1/teams/{teamId}/members/{userId}"),
    ("M6", "POST", "/api/v1/invitations/{invitationId}:accept"),
    ("M6", "POST", "/api/v1/invitations/{invitationId}:decline"),
    ("M7", "GET", "/api/v1/teams/{teamId}/expenses/{expenseId}/receipts"),
    ("M7", "POST", "/api/v1/teams/{teamId}/expenses/{expenseId}/receipts"),
    (
        "M7",
        "DELETE",
        "/api/v1/teams/{teamId}/expenses/{expenseId}/receipts/{receiptId}",
    ),
    (
        "M7",
        "GET",
        "/api/v1/teams/{teamId}/expenses/{expenseId}/receipts/{receiptId}/download",
    ),
    ("M8", "POST", "/api/v1/teams/{teamId}/expenses:batch"),
    ("M8", "GET", "/api/v1/teams/{teamId}/expenses.csv"),
    ("M8", "POST", "/api/v1/teams/{teamId}/imports"),
    ("M8", "GET", "/api/v1/teams/{teamId}/imports/{importId}"),
    ("M9", "GET", "/api/v1/teams/{teamId}/audit-events"),
];

type Operations = BTreeSet<(String, String)>;

/// `(METHOD, full path)` of every operation in the spec, with its server prefix: `/api/v1`, or the path's own
/// `servers` entry (the JWKS document lives at the root).
fn documented() -> Operations {
    let spec: Value = serde_saphyr::from_str(SPEC).expect("api/openapi.yaml is valid YAML");
    let prefix = |item: &Value| {
        item["servers"][0]["url"]
            .as_str()
            .map(|url| url.trim_end_matches('/').to_owned())
    };
    let default = prefix(&spec).expect("the spec names a server");
    let mut operations = Operations::new();
    for (path, item) in spec["paths"].as_object().expect("the spec has paths") {
        let base = prefix(item).unwrap_or_else(|| default.clone());
        for method in METHODS {
            if item.get(method).is_some() {
                operations.insert((method.to_ascii_uppercase(), format!("{base}{path}")));
            }
        }
    }
    operations
}

/// `(METHOD, template)` of every endpoint in the route table.
async fn served() -> Operations {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://ledgerly@127.0.0.1:9/ledgerly")
        .expect("a lazy pool needs no database");
    let environment = HashMap::from([
        ("SPRING_PROFILES_ACTIVE", "test"),
        ("SPRING_DATASOURCE_URL", "jdbc:postgresql://localhost:5432/ledgerly"),
        ("SPRING_DATASOURCE_USERNAME", "ledgerly"),
        ("SPRING_DATASOURCE_PASSWORD", "ledgerly"),
        ("LEDGERLY_STORAGE_S3_ENDPOINT", "http://localhost:8333"),
        ("LEDGERLY_STORAGE_S3_BUCKET", "test-files"),
        ("LEDGERLY_STORAGE_S3_ACCESS_KEY", "test"),
        ("LEDGERLY_STORAGE_S3_SECRET_KEY", "test-secret"),
        ("LEDGERLY_RECEIPTS_CLAMAV_HOST", "localhost"),
        ("LEDGERLY_RECEIPTS_CLAMAV_PORT", "3310"),
    ]);
    let config = Config::from_environment(&environment).expect("the test profile's settings are complete");
    let modules = Modules::new(&config, &pool).expect("the test profile generates a signing key");
    ledgerly_app::routes(&Limits::default(), &modules, &pool)
        .operations()
        .map(|(method, template)| (method.as_str().to_owned(), template.to_owned()))
        .collect()
}

/// What is served but undocumented, and what is documented but not served.
fn drift(served: &Operations, documented: &Operations) -> (Vec<String>, Vec<String>) {
    let show = |(method, path): &(String, String)| format!("{method} {path}");
    (
        served.difference(documented).map(show).collect(),
        documented.difference(served).map(show).collect(),
    )
}

#[tokio::test]
async fn every_served_endpoint_is_documented_and_every_documented_one_is_served_or_scheduled() {
    let served = served().await;
    let documented = documented();
    assert!(documented.len() > 30, "{}", documented.len());

    let (undocumented, unserved) = drift(&served, &documented);

    assert!(
        undocumented.is_empty(),
        "served but not in api/openapi.yaml: {undocumented:#?}"
    );
    let scheduled: Vec<String> = NOT_YET
        .iter()
        .map(|(_, method, path)| format!("{method} {path}"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(
        unserved, scheduled,
        "documented but not served, versus the operations scheduled for later"
    );
}

/// Java: `Api9ImproperInventoryManagementTest.theComparisonDetectsDriftInBothDirections`.
#[test]
fn the_comparison_detects_drift_in_both_directions() {
    let operation = |method: &str, path: &str| (method.to_owned(), path.to_owned());
    let documented = Operations::from([operation("GET", "/api/v1/a"), operation("POST", "/api/v1/b")]);
    let served = Operations::from([operation("GET", "/api/v1/a"), operation("DELETE", "/api/v1/hidden")]);

    assert_eq!(
        drift(&served, &documented),
        (
            vec!["DELETE /api/v1/hidden".to_owned()],
            vec!["POST /api/v1/b".to_owned()]
        )
    );
}
