//! Turns what `cargo nextest` reports into the suite's progress meter: passed, ignored and failed tests per
//! milestone (CONCEPT §6).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::Value;

const PACKAGE: &str = "ledgerly-parity-tests";

/// One test of the suite: the file under `tests/` it is in, and its name inside that file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TestId {
    pub file: String,
    pub name: String,
}

/// A test as `cargo nextest list` knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub id: TestId,
    pub ignored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub passed: usize,
    pub ignored: usize,
    pub failed: usize,
}

/// Reads the output of `cargo nextest list --message-format json` and keeps the tests under `tests/` of this
/// package. The harness's own unit tests are not parity tests.
///
/// # Errors
///
/// Returns a description if the text is not the JSON nextest writes.
pub fn parse_list(json: &str) -> Result<Vec<Listed>, String> {
    let document: Value = serde_json::from_str(json).map_err(|error| format!("the test list is not JSON: {error}"))?;
    let suites = document["rust-suites"]
        .as_object()
        .ok_or("the test list has no `rust-suites`")?;
    let mut listed = Vec::new();
    for suite in suites.values() {
        if suite["package-name"] != PACKAGE || suite["kind"] != "test" {
            continue;
        }
        let file = suite["binary-name"].as_str().ok_or("a suite has no `binary-name`")?;
        let cases = suite["testcases"].as_object().ok_or("a suite has no `testcases`")?;
        for (name, case) in cases {
            listed.push(Listed {
                id: TestId {
                    file: file.to_owned(),
                    name: name.clone(),
                },
                ignored: case["ignored"].as_bool().unwrap_or(false),
            });
        }
    }
    listed.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(listed)
}

/// Reads the lines `cargo nextest run --message-format libtest-json` writes and keeps the result of every test.
/// Lines that are not test results (suite summaries, anything that is not JSON) are skipped.
#[must_use]
pub fn parse_events(lines: &str) -> Vec<(TestId, Outcome)> {
    lines
        .lines()
        .filter_map(|line| {
            let event: Value = serde_json::from_str(line).ok()?;
            if event["type"] != "test" {
                return None;
            }
            let outcome = match event["event"].as_str()? {
                "ok" => Outcome::Passed,
                "failed" => Outcome::Failed,
                _ => return None,
            };
            // `<package>::<binary>$<test name>`
            let (binary, name) = event["name"].as_str()?.split_once('$')?;
            let file = binary.strip_prefix(PACKAGE)?.strip_prefix("::")?;
            Some((
                TestId {
                    file: file.to_owned(),
                    name: name.to_owned(),
                },
                outcome,
            ))
        })
        .collect()
}

/// The milestone a test belongs to: the number of the `m<N>` module it is in.
#[must_use]
pub fn milestone(test_name: &str) -> Option<u32> {
    test_name
        .split("::")
        .find_map(|segment| segment.strip_prefix('m')?.parse().ok())
}

/// Counts per milestone. A test that is not ignored and has no result did not run (its app never became
/// ready, for instance) and counts as failed. A test outside any `m<N>` module is counted under `None`.
#[must_use]
pub fn tally(listed: &[Listed], outcomes: &[(TestId, Outcome)]) -> BTreeMap<Option<u32>, Counts> {
    let outcomes: BTreeMap<&TestId, Outcome> = outcomes.iter().map(|(id, outcome)| (id, *outcome)).collect();
    let mut counts: BTreeMap<Option<u32>, Counts> = BTreeMap::new();
    for test in listed {
        let entry = counts.entry(milestone(&test.id.name)).or_default();
        match (test.ignored, outcomes.get(&test.id)) {
            (_, Some(Outcome::Passed)) => entry.passed += 1,
            (true, None) => entry.ignored += 1,
            (_, Some(Outcome::Failed)) | (false, None) => entry.failed += 1,
        }
    }
    counts
}

/// The table `just parity` prints.
#[must_use]
pub fn render(counts: &BTreeMap<Option<u32>, Counts>) -> String {
    let mut table = String::from("milestone   passed  ignored   failed\n");
    let mut total = Counts::default();
    for (milestone, count) in counts {
        let label = milestone.map_or_else(|| "untagged".to_owned(), |number| format!("M{number}"));
        let _ = writeln!(
            table,
            "{label:<9} {:>8} {:>8} {:>8}",
            count.passed, count.ignored, count.failed
        );
        total.passed += count.passed;
        total.ignored += count.ignored;
        total.failed += count.failed;
    }
    let _ = writeln!(
        table,
        "{:<9} {:>8} {:>8} {:>8}",
        "total", total.passed, total.ignored, total.failed
    );
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(file: &str, name: &str) -> TestId {
        TestId {
            file: file.to_owned(),
            name: name.to_owned(),
        }
    }

    const LIST: &str = r#"{
      "rust-suites": {
        "ledgerly-parity-tests": {
          "package-name": "ledgerly-parity-tests", "kind": "lib", "binary-name": "ledgerly_parity_tests",
          "testcases": {"spec::tests::formats_are_checked": {"ignored": false}}
        },
        "ledgerly-parity-tests::operational": {
          "package-name": "ledgerly-parity-tests", "kind": "test", "binary-name": "operational",
          "testcases": {
            "m1::health_is_up": {"ignored": false},
            "m10::prometheus_is_served": {"ignored": true}
          }
        },
        "ledgerly-app::probes": {
          "package-name": "ledgerly-app", "kind": "test", "binary-name": "probes",
          "testcases": {"health": {"ignored": false}}
        }
      }
    }"#;

    #[test]
    fn the_list_keeps_only_the_parity_tests() {
        assert_eq!(
            parse_list(LIST).unwrap(),
            [
                Listed {
                    id: id("operational", "m10::prometheus_is_served"),
                    ignored: true
                },
                Listed {
                    id: id("operational", "m1::health_is_up"),
                    ignored: false
                },
            ]
        );
        assert!(parse_list("not json").is_err());
        assert!(parse_list("{}").is_err());
    }

    #[test]
    fn results_are_read_from_the_test_events() {
        let lines = concat!(
            "{\"type\":\"suite\",\"event\":\"started\",\"test_count\":2}\n",
            "{\"type\":\"test\",\"event\":\"started\",\"name\":\"ledgerly-parity-tests::operational$m1::a\"}\n",
            "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"ledgerly-parity-tests::operational$m1::a\"}\n",
            "{\"type\":\"test\",\"event\":\"failed\",\"name\":\"ledgerly-parity-tests::operational$m1::b\"}\n",
            "Compiling something\n",
            "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"ledgerly-app::probes$health\"}\n",
        );

        assert_eq!(
            parse_events(lines),
            [
                (id("operational", "m1::a"), Outcome::Passed),
                (id("operational", "m1::b"), Outcome::Failed)
            ]
        );
    }

    #[test]
    fn the_milestone_is_the_number_of_the_m_module() {
        assert_eq!(milestone("m1::health_is_up"), Some(1));
        assert_eq!(milestone("m10::metrics::prometheus"), Some(10));
        assert_eq!(milestone("traps::m6::status_precedence"), Some(6));
        assert_eq!(milestone("helpers::tests::something"), None);
        assert_eq!(milestone("money::rounding"), None);
    }

    #[test]
    fn tests_are_counted_per_milestone() {
        let listed = [
            Listed {
                id: id("a", "m1::passes"),
                ignored: false,
            },
            Listed {
                id: id("a", "m1::fails"),
                ignored: false,
            },
            Listed {
                id: id("a", "m1::never_ran"),
                ignored: false,
            },
            Listed {
                id: id("b", "m5::later"),
                ignored: true,
            },
            Listed {
                id: id("b", "untagged"),
                ignored: true,
            },
        ];
        let outcomes = [
            (id("a", "m1::passes"), Outcome::Passed),
            (id("a", "m1::fails"), Outcome::Failed),
        ];

        let counts = tally(&listed, &outcomes);

        assert_eq!(
            counts[&Some(1)],
            Counts {
                passed: 1,
                ignored: 0,
                failed: 2
            }
        );
        assert_eq!(
            counts[&Some(5)],
            Counts {
                passed: 0,
                ignored: 1,
                failed: 0
            }
        );
        assert_eq!(counts[&None].ignored, 1);
    }

    #[test]
    fn the_table_lists_milestones_in_order_with_a_total() {
        let counts = BTreeMap::from([
            (
                Some(10),
                Counts {
                    passed: 0,
                    ignored: 7,
                    failed: 0,
                },
            ),
            (
                Some(1),
                Counts {
                    passed: 5,
                    ignored: 0,
                    failed: 1,
                },
            ),
        ]);

        assert_eq!(
            render(&counts),
            "milestone   passed  ignored   failed\n\
             M1               5        0        1\n\
             M10              0        7        0\n\
             total            5        7        1\n"
        );
    }
}
