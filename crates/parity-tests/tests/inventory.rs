//! Every Java test of `docs/reference/java-test-inventory.md` is accounted for: each test method of §1 and
//! each test class of §2 is named by a parity test (`Java: \`Class.method\``) or in `NOT_PARITY.md`
//! (plan M1.10). This test needs no running app.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::Path;

const INVENTORY: &str = include_str!("../../../docs/reference/java-test-inventory.md");
const NOT_PARITY: &str = include_str!("../NOT_PARITY.md");

/// The text of §`number` of the inventory, up to the next `## `.
fn section(number: &str) -> &'static str {
    let start = INVENTORY
        .find(&format!("\n## {number}. "))
        .unwrap_or_else(|| panic!("no section {number}"));
    let rest = &INVENTORY[start + 1..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |end| end + 3);
    &rest[..end]
}

/// `(class, method)` for every row of the tables in §1.
fn section_one_methods() -> Vec<(String, String)> {
    let mut class = String::new();
    let mut methods = Vec::new();
    for line in section("1").lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            class = heading.split(".java").next().unwrap().trim().to_owned();
        } else if let Some(row) = line.strip_prefix("| `") {
            let method = row.split('`').next().unwrap().to_owned();
            methods.push((class.clone(), method));
        }
    }
    methods
}

/// The simple name of every test class named in §2, such as `AuthApiTest`.
fn section_two_classes() -> Vec<String> {
    let mut classes: Vec<String> = section("2")
        .split('`')
        .filter(|token| token.ends_with("Test.java"))
        .map(|path| path.rsplit('/').next().unwrap().trim_end_matches(".java").to_owned())
        .collect();
    classes.sort();
    classes.dedup();
    classes
}

/// The text of every parity test file.
fn parity_sources() -> String {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_inventory_is_read_as_expected() {
    assert_eq!(section_one_methods().len(), 61);
    let classes = section_two_classes();
    assert!(classes.contains(&"AuthApiTest".to_owned()) && classes.contains(&"UuidV7Test".to_owned()));
    assert_eq!(classes.len(), 43, "{classes:?}");
}

#[test]
fn every_owasp_test_method_is_a_parity_test_or_listed_as_not_parity() {
    let sources = parity_sources();
    let missing: Vec<String> = section_one_methods()
        .into_iter()
        .map(|(class, method)| format!("{class}.{method}"))
        .filter(|name| !sources.contains(&format!("`{name}")) && !NOT_PARITY.contains(&format!("`{name}")))
        .collect();

    assert!(
        missing.is_empty(),
        "not ported and not in NOT_PARITY.md:\n{}",
        missing.join("\n")
    );
}

#[test]
fn every_other_test_class_is_ported_or_listed_as_not_parity() {
    let sources = parity_sources();
    let missing: Vec<String> = section_two_classes()
        .into_iter()
        .filter(|class| !sources.contains(&format!("`{class}.")) && !NOT_PARITY.contains(class.as_str()))
        .collect();

    assert!(
        missing.is_empty(),
        "not ported and not in NOT_PARITY.md:\n{}",
        missing.join("\n")
    );
}

#[test]
fn every_java_test_named_by_a_parity_test_exists_in_the_inventory() {
    // Guards against a typo in a `Java:` comment that would make the checks above pass by accident.
    let known_classes: Vec<String> = section_one_methods()
        .into_iter()
        .map(|(class, _)| class)
        .chain(section_two_classes())
        .collect();
    let sources = parity_sources();
    let unknown: Vec<&str> = sources
        .match_indices("Java: `")
        .filter_map(|(at, _)| sources[at + 7..].split('`').next())
        .filter(|name| name.contains('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_'))
        .map(|name| name.split('.').next().unwrap())
        .filter(|class| !known_classes.iter().any(|known| known == class))
        .collect();

    assert!(unknown.is_empty(), "classes not in the inventory: {unknown:?}");
}
