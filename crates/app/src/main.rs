//! The `ledgerly` binary. It only starts what the library crate wires together.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use ledgerly_app::config::{Config, ProcessEnvironment};
use ledgerly_app::{App, stop_signal};

fn main() -> ExitCode {
    let config = match Config::from_environment(&ProcessEnvironment) {
        Ok(config) => config,
        Err(error) => return failed_to_start(&error),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => return failed_to_start(&error),
    };
    match runtime.block_on(run(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => failed_to_start(&error),
    }
}

async fn run(config: Config) -> std::io::Result<()> {
    let stop = stop_signal()?;
    let app = App::bind(config).await?;
    app.serve(stop).await
}

/// Reports why the process stops, readable without the logging system. The banner is the one Spring Boot
/// prints for a failure analysis.
fn failed_to_start(error: &dyn std::fmt::Display) -> ExitCode {
    eprintln!("\n***************************\nAPPLICATION FAILED TO START\n***************************\n\n{error}\n");
    ExitCode::FAILURE
}
