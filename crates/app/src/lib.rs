//! Wires the modules into the `ledgerly` binary: configuration, router, layers, seeding and shutdown.
//!
//! Java counterpart: `LedgerlyApplication` and the Spring context, in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

pub mod config;
mod management;
mod server;

use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use axum::Router;
use tokio::net::TcpListener;

use crate::config::Config;
pub use crate::management::BuildInfo;
use crate::server::Shutdown;

/// The API router: everything served on the public port.
///
/// Tests and the binary build it with this one function, so that both run the same routes and layers. It has
/// no routes yet; an unknown path is a bare 404 until M3 gives it Java's problem body.
pub fn router(_config: &Config) -> Router {
    Router::new()
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
}

impl App {
    /// Binds the API port and the management port on every interface, like Spring Boot's default.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if a port cannot be bound, for example because it is in use.
    pub async fn bind(config: Config) -> io::Result<Self> {
        let api = TcpListener::bind((Ipv4Addr::UNSPECIFIED, config.server.api_port)).await?;
        let management = TcpListener::bind((Ipv4Addr::UNSPECIFIED, config.server.management_port)).await?;
        Ok(Self {
            api,
            management,
            config,
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
        let api_shutdown = Shutdown::new();
        let management_shutdown = Shutdown::new();

        let api = server::serve(self.api, router(&self.config), api_shutdown.clone(), drain);
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
            api_shutdown.start();
        };

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
