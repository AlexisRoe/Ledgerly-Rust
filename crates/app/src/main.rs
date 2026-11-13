//! The `ledgerly` binary. It only starts what the library crate wires together.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use ledgerly_app::config::{Config, ProcessEnvironment};
use ledgerly_app::telemetry::Telemetry;
use ledgerly_app::{App, logging, stop_signal};
use ledgerly_shared::database;

fn main() -> ExitCode {
    let config = match Config::from_environment(&ProcessEnvironment) {
        Ok(config) => config,
        Err(error) => return failed_to_start(&error),
    };
    if let Err(error) = logging::init(config.profiles) {
        return failed_to_start(&error);
    }
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
    let app = App::bind(config, telemetry).await?;
    app.serve(stop).await?;
    // Nothing reads or writes through the pool until the use cases are wired into the router (M3). It is opened
    // and checked here so that a wrong database stops the start, and is closed after the last request.
    pool.close().await;
    Ok(())
}

/// Why `run` ended with an error. None of the messages contains a secret: the configuration was already
/// redacted, and `sqlx` names the host and user of a failed connection, never the password.
#[derive(Debug, thiserror::Error)]
enum StartupError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to connect to the database: {0}")]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Schema(#[from] database::SchemaError),
}

/// Reports why the process stops, readable without the logging system. The banner is the one Spring Boot
/// prints for a failure analysis.
fn failed_to_start(error: &dyn std::fmt::Display) -> ExitCode {
    eprintln!("\n***************************\nAPPLICATION FAILED TO START\n***************************\n\n{error}\n");
    ExitCode::FAILURE
}
