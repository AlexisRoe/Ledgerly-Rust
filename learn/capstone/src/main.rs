//! Starts the link shortener on `127.0.0.1:$PORT` (default 3000). Ctrl-C stops it gracefully.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use shortlinks::store::{LinkStore, MemoryStore};
use shortlinks::visits;
use shortlinks::web::{AppState, router};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> ExitCode {
    let _ = tracing_subscriber::fmt().try_init();
    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(3000_u16);
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, port, "cannot listen");
            return ExitCode::FAILURE;
        }
    };

    let store: Arc<dyn LinkStore> = Arc::new(MemoryStore::default());
    let (sender, receiver) = mpsc::channel(1024);
    let counter = tokio::spawn(visits::run(Arc::clone(&store), receiver, Duration::from_secs(1)));
    let app = router(AppState::new(store, sender));

    tracing::info!(port, "listening");
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await;
    // `serve` has returned and dropped the router, and with it the last visit sender: the counter writes what's
    // left and ends.
    let _ = counter.await;

    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "server failed");
            ExitCode::FAILURE
        }
    }
}
