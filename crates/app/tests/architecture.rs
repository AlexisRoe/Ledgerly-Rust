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
    // Generated from the OpenAPI file; knows nothing about the modules.
    ("api", &[]),
    (
        "app",
        &[
            "shared", "identity", "teams", "expenses", "receipts", "imports", "audit", "api",
        ],
    ),
    // Black box: it may only talk HTTP and SQL (ADR 0011).
    ("parity-tests", &[]),
];

/// Crates that may additionally depend on `api`, because their `web` layer implements the generated traits.
/// The arrow is allowed, not required: a module gets it in the milestone that gives it endpoints.
const MAY_USE_API: &[&str] = &["identity", "teams", "expenses", "receipts", "imports", "audit"];

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
            let allowed =
                wanted.contains(dependency.as_str()) || (dependency == "api" && MAY_USE_API.contains(&name.as_str()));
            if !allowed {
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

#[test]
fn modules_may_depend_on_the_generated_api_but_shared_may_not() {
    let mut edges = REQUIRED.to_vec();
    edges.retain(|(name, _)| !["expenses", "shared"].contains(name));
    edges.push(("expenses", &["shared", "teams", "api"]));
    edges.push(("shared", &["api"]));

    assert_eq!(
        violations(&graph(&edges)),
        ["`shared` depends on `api`, which CONCEPT §3 does not allow; \
          invert the call with a trait the lower crate owns"]
    );
}
