//! Wires the modules into the `ledgerly` binary: configuration, router, layers, seeding and shutdown.
//!
//! Java counterpart: `LedgerlyApplication` and the Spring context, in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

pub mod config;
pub mod logging;
mod management;
mod server;
pub mod telemetry;
#[cfg(test)]
mod tests;

use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use std::any::Any;

use axum::Router;
use axum::response::IntoResponse;
use ledgerly_shared::web::{self, Problem, Routes};
use tokio::net::TcpListener;
use tower_http::catch_panic::CatchPanicLayer;

use crate::config::Config;
pub use crate::management::BuildInfo;
use crate::server::Shutdown;
use crate::telemetry::Telemetry;

/// The API router: everything served on the public port.
///
/// Tests and the binary build it with this one function, so that both run the same routes and layers. The
/// routes come from one table ([`Routes`]), which the inventory test compares with the spec. It is still empty;
/// the expense endpoints arrive in M3.4.
pub fn router(_config: &Config, telemetry: &Telemetry) -> Router {
    layered(routes(), telemetry)
}

/// Every endpoint the API serves.
#[must_use]
pub fn routes() -> Routes {
    Routes::new()
}

/// `routes` behind the layers every API request passes, from the outside in:
///
/// 1. tracing: a span with `traceId` and `spanId` for the request and its log lines;
/// 2. problem rendering: every problem body gets `instance` and `traceId`;
/// 3. panic catching: a handler that panics is answered with a 500 `INTERNAL_ERROR` problem that carries only
///    the `traceId`, and logged as Java logs an unhandled exception.
///
/// Public so that tests can put a route of their own behind exactly these layers.
pub fn layered(routes: Routes, telemetry: &Telemetry) -> Router {
    Router::new()
        .fallback_service(routes.into_service())
        .layer(CatchPanicLayer::custom(unhandled_panic))
        .layer(axum::middleware::from_fn(web::render))
        .layer(axum::middleware::from_fn_with_state(
            telemetry.tracer(),
            telemetry::trace_request,
        ))
}

/// The answer to a handler that panicked. What it panicked with is not logged: a panic message may quote any
/// value, and Java's log line names only the exception class. The trace id finds the request.
fn unhandled_panic(_panic: Box<dyn Any + Send + 'static>) -> axum::response::Response {
    tracing::error!("Unhandled exception");
    Problem::internal_error().into_response()
}

/// The service with both listeners bound, ready to serve.
///
/// Binding and serving are separate steps so that a caller who asked for port 0 can learn the real ports
/// before the first request.
#[derive(Debug)]
pub struct App {
    api: TcpListener,
    management: TcpListener,
    config: Config,
    telemetry: Telemetry,
}

impl App {
    /// Binds the API port and the management port on every interface, like Spring Boot's default.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if a port cannot be bound, for example because it is in use.
    pub async fn bind(config: Config, telemetry: Telemetry) -> io::Result<Self> {
        let api = TcpListener::bind((Ipv4Addr::UNSPECIFIED, config.server.api_port)).await?;
        let management = TcpListener::bind((Ipv4Addr::UNSPECIFIED, config.server.management_port)).await?;
        Ok(Self {
            api,
            management,
            config,
            telemetry,
        })
    }

    /// The address the API listens on.
    ///
    /// # Errors
    ///
    /// Returns the I/O error of the operating system if the socket has no address.
    pub fn api_address(&self) -> io::Result<SocketAddr> {
        self.api.local_addr()
    }

    /// The address the management endpoints listen on.
    ///
    /// # Errors
    ///
    /// Returns the I/O error of the operating system if the socket has no address.
    pub fn management_address(&self) -> io::Result<SocketAddr> {
        self.management.local_addr()
    }

    /// Serves until `stop` completes, then shuts down gracefully: readiness turns `OUT_OF_SERVICE`, the API
    /// accepts no new connection, and requests in flight get the configured time to finish. The management
    /// listener keeps answering probes until the API has drained, and stops last.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if a listener fails while serving.
    pub async fn serve(self, stop: impl Future<Output = ()>) -> io::Result<()> {
        let drain = self.config.server.shutdown_timeout;
        let api_port = self.api.local_addr()?.port();
        let management_port = self.management.local_addr()?.port();
        let api_shutdown = Shutdown::new();
        let management_shutdown = Shutdown::new();

        let api = server::serve(
            self.api,
            router(&self.config, &self.telemetry),
            api_shutdown.clone(),
            drain,
        );
        let management = server::serve(
            self.management,
            management::router(api_shutdown.clone(), BuildInfo::current()),
            management_shutdown.clone(),
            drain,
        );
        let api_then_management = async {
            let result = api.await;
            management_shutdown.start();
            result
        };
        let trigger = async {
            stop.await;
            tracing::info!("Commencing graceful shutdown. Waiting for active requests to complete");
            api_shutdown.start();
        };

        tracing::info!(apiPort = api_port, managementPort = management_port, "Started Ledgerly");
        let (api_result, management_result, ()) = tokio::join!(api_then_management, management, trigger);
        api_result.and(management_result)
    }
}

/// Completes when the process is asked to stop: SIGTERM from an orchestrator, or Ctrl-C in a terminal.
///
/// # Errors
///
/// Returns the I/O error if the signal handlers cannot be installed.
pub fn stop_signal() -> io::Result<impl Future<Output = ()>> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    Ok(async move {
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
    })
}
