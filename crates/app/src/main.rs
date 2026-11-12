//! The `ledgerly` binary. It only starts what the library crate wires together.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use ledgerly_app::config::{Config, ProcessEnvironment};

fn main() -> ExitCode {
    match Config::from_environment(&ProcessEnvironment) {
        // Nothing to start yet: the listeners arrive in M1.3.
        Ok(_config) => ExitCode::SUCCESS,
        Err(error) => {
            // The logging system is not up yet, and this has to be readable without it. The banner is the one
            // Spring Boot prints for a failure analysis.
            eprintln!(
                "\n***************************\nAPPLICATION FAILED TO START\n***************************\n\n{error}\n"
            );
            ExitCode::FAILURE
        }
    }
}
