//! The `ledgerly` binary. It only starts what the library crate wires together.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use ledgerly_app::config::{Config, ProcessEnvironment};
use ledgerly_app::telemetry::Telemetry;
use ledgerly_app::{App, StartupError, logging, seed, stop_signal};
use ledgerly_shared::database;

fn main() -> ExitCode {
    let config = match Config::from_environment(&ProcessEnvironment) {
        Ok(config) => config,
        Err(error) => return failed_to_start(&error),
    };
    if let Err(error) = logging::init(config.profiles) {
        return failed_to_start(&error);
    }
    // Rust's default hook prints the panic message to stderr, outside the log format, and a message may quote any
    // value. This one logs where it happened, which is enough to find the bug; a panic in a request also gets
    // the API's "Unhandled exception" line with the trace id.
    std::panic::set_hook(Box::new(|panic| {
        let location = panic.location().map(ToString::to_string).unwrap_or_default();
        tracing::error!(location, "Thread panicked");
    }));
    // Before the runtime exists: the exporter's HTTP client is a blocking one.
    let telemetry = match Telemetry::new(&config.tracing) {
        Ok(telemetry) => telemetry,
        Err(error) => return failed_to_start(&error),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => return failed_to_start(&error),
    };
    let result = runtime.block_on(run(config, telemetry.clone()));
    // Stopping the runtime ends any request that outlasted the drain time.
    drop(runtime);
    telemetry.shutdown();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => failed_to_start(&error),
    }
}

async fn run(config: Config, telemetry: Telemetry) -> Result<(), StartupError> {
    let stop = stop_signal()?;
    let pool = database::connect(&config.database).await?;
    // Before the ports open: a schema this build was not made for is never served (ADR 0006).
    database::verify(&pool).await?;
    // Java's `DevDataSeeder` (`local` without `test`): an empty database gets the demo team.
    seed::demo_data(config.profiles, &pool, chrono::Utc::now()).await?;
    let app = App::bind(config, telemetry, pool.clone()).await?;
    app.serve(stop).await?;
    // Closed after the last request has finished.
    pool.close().await;
    Ok(())
}

/// Reports why the process stops, readable without the logging system. The banner is the one Spring Boot
/// prints for a failure analysis.
fn failed_to_start(error: &dyn std::fmt::Display) -> ExitCode {
    eprintln!("\n***************************\nAPPLICATION FAILED TO START\n***************************\n\n{error}\n");
    ExitCode::FAILURE
}
