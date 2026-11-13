//! `just parity`: runs the parity suite and prints passed / ignored / failed per milestone.
//!
//! Without an argument it starts the app container once per run group (`groups.rs`) with that group's
//! environment and runs the group's test files against it. With a base URL it runs only the default group
//! against a deployment that is already running, Ledgerly-Rust or Ledgerly-Java (ADR 0011).

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use ledgerly_parity_tests::groups::{DEFAULT, GROUPS, Group, environment_of, group_of};
use ledgerly_parity_tests::report::{Listed, Outcome, TestId, parse_events, parse_list, render, tally};

/// Host ports of the app container under test. Not 8080/8081, so that `just up` can stay running.
const API_PORT: u16 = 18080;
const MANAGEMENT_PORT: u16 = 18081;
const PACKAGE: &str = "ledgerly-parity-tests";

fn main() -> ExitCode {
    let existing_deployment = std::env::args().nth(1).filter(|url| !url.trim().is_empty());
    match run(existing_deployment.as_deref()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("parity: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Returns whether no test failed.
fn run(existing_deployment: Option<&str>) -> Result<bool, String> {
    let listed = list_tests()?;
    let mut outcomes: Vec<(TestId, Outcome)> = Vec::new();

    for group in GROUPS {
        let files: BTreeSet<&str> = listed
            .iter()
            .filter(|test| !test.ignored && group_of(&test.id.file).name == group.name)
            .map(|test| test.id.file.as_str())
            .collect();
        if files.is_empty() {
            // Nothing to run: every test of the group is still ignored. No need to start the app for it.
            continue;
        }
        match existing_deployment {
            Some(base_url) if group.name == DEFAULT.name => {
                eprintln!("parity: group `{}` against {base_url}", group.name);
                outcomes.extend(run_tests(&files, base_url, None)?);
            }
            Some(_) => eprintln!(
                "parity: group `{}` skipped: it needs the app started with its own configuration",
                group.name
            ),
            None => {
                eprintln!("parity: group `{}`: starting the app", group.name);
                let container = start_app(group)?;
                let result = wait_until_ready().and_then(|()| {
                    run_tests(
                        &files,
                        &format!("http://127.0.0.1:{API_PORT}"),
                        Some(&format!("http://127.0.0.1:{MANAGEMENT_PORT}")),
                    )
                });
                if result.is_err() {
                    print_logs(&container);
                }
                stop_app(&container);
                outcomes.extend(result?);
            }
        }
    }

    let counts = tally(&for_report(listed, existing_deployment.is_some()), &outcomes);
    println!("\n{}", render(&counts));
    Ok(counts.values().all(|count| count.failed == 0))
}

/// Against an existing deployment only the default group runs, so only its tests are reported.
fn for_report(listed: Vec<Listed>, default_group_only: bool) -> Vec<Listed> {
    listed
        .into_iter()
        .filter(|test| !default_group_only || group_of(&test.id.file).name == DEFAULT.name)
        .collect()
}

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
}

fn list_tests() -> Result<Vec<Listed>, String> {
    let output = cargo()
        .args(["nextest", "list", "--locked", "-p", PACKAGE, "--message-format", "json"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("cargo nextest could not be started: {error}"))?;
    if !output.status.success() {
        return Err("cargo nextest list failed".to_owned());
    }
    parse_list(&String::from_utf8_lossy(&output.stdout))
}

/// Runs the test files of one group. A failing test is a result, not an error.
fn run_tests(
    files: &BTreeSet<&str>,
    base_url: &str,
    management_url: Option<&str>,
) -> Result<Vec<(TestId, Outcome)>, String> {
    let filter = files
        .iter()
        .map(|file| format!("binary(={file})"))
        .collect::<Vec<_>>()
        .join(" | ");
    let mut command = cargo();
    command
        .args(["nextest", "run", "--locked", "-p", PACKAGE, "--no-fail-fast"])
        .args(["--message-format", "libtest-json", "-E", &filter])
        .env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1")
        .env("PARITY_BASE_URL", base_url)
        .stderr(Stdio::inherit());
    if let Some(management_url) = management_url {
        command.env("PARITY_MANAGEMENT_URL", management_url);
    }
    let output = command
        .output()
        .map_err(|error| format!("cargo nextest could not be started: {error}"))?;
    Ok(parse_events(&String::from_utf8_lossy(&output.stdout)))
}

fn container_name(group: &Group) -> String {
    format!("ledgerly-parity-{}", group.name)
}

/// Starts the app service of `compose.yaml` (and whatever it depends on: Postgres, Flyway, the object store,
/// the scanner) with the group's environment. Returns the container's name.
fn start_app(group: &Group) -> Result<String, String> {
    let name = container_name(group);
    // A container left over from an interrupted run would keep the name and the ports.
    stop_app(&name);
    let mut command = Command::new("docker");
    command
        .args([
            "compose",
            "--profile",
            "app",
            "run",
            "--detach",
            "--rm",
            "--name",
            &name,
        ])
        .args(["--publish", &format!("127.0.0.1:{API_PORT}:8080")])
        .args(["--publish", &format!("127.0.0.1:{MANAGEMENT_PORT}:8081")]);
    for (variable, value) in environment_of(group) {
        command.args(["--env", &format!("{variable}={value}")]);
    }
    let status = command
        .arg("app")
        .stdout(Stdio::null())
        .status()
        .map_err(|error| format!("docker could not be started: {error}"))?;
    if status.success() {
        Ok(name)
    } else {
        Err(format!(
            "the app container for group `{}` did not start. Is Docker running, and was the image built (`just image`)?",
            group.name
        ))
    }
}

fn stop_app(container: &str) {
    // Stopping a container that does not exist fails, which is fine.
    let _ = Command::new("docker")
        .args(["rm", "--force", container])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn print_logs(container: &str) {
    eprintln!("parity: logs of {container}:");
    let _ = Command::new("docker")
        .args(["logs", "--tail", "50", container])
        .status();
}

/// Waits for the readiness probe of the container just started, for at most a minute.
fn wait_until_ready() -> Result<(), String> {
    let url = format!("http://127.0.0.1:{MANAGEMENT_PORT}/actuator/health/readiness");
    let deadline = Instant::now() + Duration::from_secs(60);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("no runtime for the readiness check: {error}"))?;
    loop {
        let ready = runtime.block_on(async {
            match reqwest::Client::new()
                .get(&url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
            {
                Ok(response) => response.status().as_u16() == 200,
                Err(_) => false,
            }
        });
        if ready {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("the app was not ready after 60 s ({url})"));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}
