//! The crate dependency graph is the module diagram of CONCEPT §3. Java checks the same arrows with Spring
//! Modulith's `ApplicationModules.verify()` (`src/test/java/com/ledgerly/ModularityTest.java`).

// clippy.toml exempts `#[test]` functions only; the helpers of an integration test are test code too.
#![allow(clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

const PREFIX: &str = "ledgerly-";

/// Every crate of the workspace with the workspace crates it must depend on: no more, no fewer.
const REQUIRED: &[(&str, &[&str])] = &[
    ("shared", &[]),
    ("identity", &["shared"]),
    ("teams", &["shared", "identity"]),
    ("expenses", &["shared", "teams"]),
    ("receipts", &["shared", "expenses"]),
    ("imports", &["shared", "expenses", "teams"]),
    // Audit listens to everyone's events. It asks for roles through a trait `shared` owns, so the arrow to
    // `teams` is there for the events only.
    (
        "audit",
        &["shared", "identity", "teams", "expenses", "receipts", "imports"],
    ),
    (
        "app",
        &[
            "shared", "identity", "teams", "expenses", "receipts", "imports", "audit",
        ],
    ),
    // Black box: it may only talk HTTP and SQL (ADR 0011).
    ("parity-tests", &[]),
];

/// The edges between workspace crates as `cargo metadata` reports them, including dev- and build-dependencies.
fn workspace_graph() -> BTreeMap<String, BTreeSet<String>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata can be started");
    assert!(
        output.status.success(),
        "cargo metadata failed (a dependency cycle between crates shows up here):\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");

    let packages = metadata["packages"].as_array().expect("packages is an array");
    let short = |name: &str| name.strip_prefix(PREFIX).map(str::to_owned);
    packages
        .iter()
        .map(|package| {
            let name = package["name"].as_str().expect("a package has a name");
            let name = short(name).unwrap_or_else(|| panic!("workspace crate `{name}` is not named `{PREFIX}*`"));
            let dependencies = package["dependencies"]
                .as_array()
                .expect("dependencies is an array")
                .iter()
                .filter_map(|dependency| short(dependency["name"].as_str()?))
                // A crate may dev-depend on itself to switch on its own test features.
                .filter(|dependency| *dependency != name)
                .collect();
            (name, dependencies)
        })
        .collect()
}

/// Compares a graph with the allowed arrows and describes every difference, one per line.
fn violations(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    let mut found = Vec::new();
    let required: BTreeMap<&str, BTreeSet<&str>> = REQUIRED
        .iter()
        .map(|(name, dependencies)| (*name, dependencies.iter().copied().collect()))
        .collect();

    for (name, actual) in graph {
        let Some(wanted) = required.get(name.as_str()) else {
            found.push(format!(
                "crate `{name}` is not in the module diagram; add it to CONCEPT §3 and to this test"
            ));
            continue;
        };
        for dependency in actual {
            if !wanted.contains(dependency.as_str()) {
                found.push(format!(
                    "`{name}` depends on `{dependency}`, which CONCEPT §3 does not allow; \
                     invert the call with a trait the lower crate owns"
                ));
            }
        }
        for dependency in wanted {
            if !actual.contains(*dependency) {
                found.push(format!(
                    "`{name}` does not depend on `{dependency}`, but CONCEPT §3 has that arrow"
                ));
            }
        }
    }
    for name in required.keys() {
        if !graph.contains_key(*name) {
            found.push(format!(
                "crate `{name}` is in the module diagram but not in the workspace"
            ));
        }
    }
    found
}

#[test]
fn the_crate_graph_equals_the_allowed_arrows() {
    let found = violations(&workspace_graph());
    assert!(
        found.is_empty(),
        "the crate dependency graph differs from CONCEPT §3:\n  - {}",
        found.join("\n  - ")
    );
}

fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
    edges
        .iter()
        .map(|(name, dependencies)| {
            (
                (*name).to_owned(),
                dependencies.iter().map(|d| (*d).to_owned()).collect(),
            )
        })
        .collect()
}

#[test]
fn the_allowed_arrows_themselves_pass() {
    assert_eq!(violations(&graph(REQUIRED)), Vec::<String>::new());
}

#[test]
fn an_arrow_pointing_the_wrong_way_is_reported_by_name() {
    let mut edges = REQUIRED.to_vec();
    edges.retain(|(name, _)| *name != "receipts");
    edges.push(("receipts", &["shared", "expenses", "identity"]));

    assert_eq!(
        violations(&graph(&edges)),
        ["`receipts` depends on `identity`, which CONCEPT §3 does not allow; \
          invert the call with a trait the lower crate owns"]
    );
}

#[test]
fn a_missing_arrow_and_an_unknown_crate_are_reported() {
    let mut edges = REQUIRED.to_vec();
    edges.retain(|(name, _)| *name != "imports");
    edges.push(("imports", &["shared", "expenses"]));
    edges.push(("billing", &["shared"]));

    assert_eq!(
        violations(&graph(&edges)),
        [
            "crate `billing` is not in the module diagram; add it to CONCEPT §3 and to this test",
            "`imports` does not depend on `teams`, but CONCEPT §3 has that arrow",
        ]
    );
}

// --- OWASP API7: only infrastructure code calls out ---------------------------------------------------------------
//
// Java: `Api7ServerSideRequestForgeryTest`, an ArchUnit rule that only `..infrastructure..` and `..shared.storage..`
// may depend on HTTP or socket clients. Rust has no packages to name, so the rule is checked twice: no crate but
// the ones holding the adapters depends on a client crate, and no source file but the adapters names one. Test
// code is exempt, as in Java (`ImportOption.DoNotIncludeTests`).

/// Crates that make outbound connections, and the workspace crates that may depend on them directly. `shared`
/// holds the object-store adapter, `identity` the identity provider's (`keycloak`; Java's
/// `identity.internal.infrastructure.KeycloakResourceServerConfig`). (OTLP export goes through `opentelemetry-otlp`, which is not on this list:
/// it only ever talks to the configured endpoint.)
const CLIENT_CRATES: &[(&str, &[&str])] = &[
    ("reqwest", &[]),
    ("aws-sdk-s3", &["shared"]),
    ("aws-smithy-http-client", &["shared"]),
    ("hyper", &["identity"]),
    ("hyper-util", &["identity"]),
    ("hyper-rustls", &["identity"]),
    ("ureq", &[]),
    ("isahc", &[]),
    ("surf", &[]),
    ("attohttpc", &[]),
    ("curl", &[]),
];

/// Paths in source code that open outbound connections.
const CLIENT_PATHS: &[&str] = &[
    "reqwest",
    "aws_sdk_s3",
    "aws_smithy",
    "hyper",
    "TcpStream",
    "UdpSocket",
    "UnixStream",
    "tokio::net",
];

/// Listening is not calling out: the app binds its two ports.
const SERVER_PATHS: &[&str] = &["tokio::net::TcpListener"];

/// The adapters, which are allowed to call out: the object store, the malware scanner and the identity
/// provider (discovery and its keys).
const ADAPTERS: &[&str] = &[
    "crates/shared/src/storage/",
    "crates/receipts/src/scanner.rs",
    "crates/identity/src/keycloak/issuer.rs",
];

/// The crate that is test code as a whole (ADR 0011).
const TEST_CRATES: &[&str] = &["crates/parity-tests/"];

/// `(workspace crate, client crate)` for every normal (non-dev) dependency on a client crate.
fn client_dependencies() -> BTreeSet<(String, String)> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata can be started");
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");
    let mut found = BTreeSet::new();
    for package in metadata["packages"].as_array().expect("packages is an array") {
        let name = package["name"].as_str().expect("a package has a name");
        let name = name.strip_prefix(PREFIX).unwrap_or(name);
        for dependency in package["dependencies"].as_array().expect("dependencies is an array") {
            let normal = dependency["kind"].is_null();
            let target = dependency["name"].as_str().expect("a dependency has a name");
            if normal && CLIENT_CRATES.iter().any(|(client, _)| *client == target) {
                found.insert((name.to_owned(), target.to_owned()));
            }
        }
    }
    found
}

fn crate_violations(dependencies: &BTreeSet<(String, String)>) -> Vec<String> {
    dependencies
        .iter()
        .filter(|(name, client)| {
            name != "parity-tests"
                && !CLIENT_CRATES
                    .iter()
                    .any(|(known, allowed)| known == client && allowed.contains(&name.as_str()))
        })
        .map(|(name, client)| format!("`{name}` depends on `{client}`; only infrastructure may call out"))
        .collect()
}

/// The production code of a file: everything before its `#[cfg(test)]` module, or nothing for a file that only
/// tests (`tests.rs`, `*_tests.rs`, `testing.rs`, `doubles.rs`, anything under `tests/`: all `cfg(test)` or test-only).
fn production_code(path: &str, text: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    if matches!(file, "tests.rs" | "testing.rs" | "doubles.rs")
        || file.ends_with("_tests.rs")
        || path.contains("/tests/")
    {
        return String::new();
    }
    text.split("#[cfg(test)]").next().unwrap_or_default().to_owned()
}

/// `path: name` for every client path named outside the adapters.
fn source_violations(files: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (path, text) in files {
        if ADAPTERS
            .iter()
            .chain(TEST_CRATES)
            .any(|allowed| path.starts_with(allowed))
        {
            continue;
        }
        let mut code = production_code(path, text);
        for server in SERVER_PATHS {
            code = code.replace(server, "");
        }
        for client in CLIENT_PATHS {
            if code.contains(client) {
                found.push(format!("{path}: {client}; only infrastructure may call out"));
            }
        }
    }
    found
}

/// Every `.rs` file under `crates/*/src`, with its path relative to the workspace.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).expect("the source tree can be read") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative = path
                    .strip_prefix(root)
                    .expect("under the root")
                    .to_string_lossy()
                    .into_owned();
                out.push((
                    relative,
                    std::fs::read_to_string(&path).expect("a source file is UTF-8"),
                ));
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().expect("the workspace root exists");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).expect("crates/ can be read") {
        let src = entry.expect("a directory entry").path().join("src");
        if src.is_dir() {
            walk(&src, &root, &mut files);
        }
    }
    files
}

/// Java: `Api7ServerSideRequestForgeryTest.onlyInfrastructureMayCallOut`.
#[test]
fn only_infrastructure_may_call_out() {
    let mut found = crate_violations(&client_dependencies());
    found.extend(source_violations(&sources()));

    assert!(
        found.is_empty(),
        "outbound calls outside the adapters:\n  - {}",
        found.join("\n  - ")
    );
}

/// Java: `Api7ServerSideRequestForgeryTest.theRuleReallyFailsForAClassThatBreaksIt`, with a made-up crate graph
/// and a made-up source file.
#[test]
fn the_rule_really_fails_for_code_that_breaks_it() {
    let dependencies = BTreeSet::from([("expenses".to_owned(), "reqwest".to_owned())]);
    let files = [(
        "crates/expenses/src/domain/fetch.rs".to_owned(),
        "use reqwest::Client;\nasync fn f() { tokio::net::TcpStream::connect(\"x:1\").await; }\n".to_owned(),
    )];

    assert_eq!(
        crate_violations(&dependencies),
        ["`expenses` depends on `reqwest`; only infrastructure may call out"]
    );
    assert_eq!(
        source_violations(&files),
        [
            "crates/expenses/src/domain/fetch.rs: reqwest; only infrastructure may call out",
            "crates/expenses/src/domain/fetch.rs: TcpStream; only infrastructure may call out",
            "crates/expenses/src/domain/fetch.rs: tokio::net; only infrastructure may call out",
        ]
    );
}

/// Java: `Api7ServerSideRequestForgeryTest.theSameCodeInAnInfrastructurePackageIsAllowed`: the adapters do call
/// out, and the rule exempts them; test code is exempt too.
#[test]
fn the_same_code_in_an_adapter_or_a_test_is_allowed() {
    let scanner = sources()
        .into_iter()
        .find(|(path, _)| path == "crates/receipts/src/scanner.rs")
        .expect("the scanner exists");
    assert!(scanner.1.contains("TcpStream"), "the scanner opens a socket");
    let files = [
        scanner,
        (
            "crates/shared/src/storage/s3.rs".to_owned(),
            "use aws_sdk_s3::Client;".to_owned(),
        ),
        (
            "crates/teams/src/web/tests.rs".to_owned(),
            "use reqwest::Client;".to_owned(),
        ),
        (
            "crates/teams/src/lib.rs".to_owned(),
            "pub fn f() {}\n#[cfg(test)]\nmod tests { use reqwest::Client; }".to_owned(),
        ),
    ];

    assert_eq!(source_violations(&files), Vec::<String>::new());
    assert_eq!(
        crate_violations(&BTreeSet::from([("shared".to_owned(), "aws-sdk-s3".to_owned())])),
        Vec::<String>::new()
    );
}
