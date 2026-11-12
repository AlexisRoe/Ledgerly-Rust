//! Serving a router on a listener until shutdown, letting requests in flight finish.
//!
//! Java counterpart: `server.shutdown: graceful` with `spring.lifecycle.timeout-per-shutdown-phase` in
//! `application.yaml`.

use std::io;
use std::time::Duration;

use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::watch;

/// The switch that starts a shutdown. Cloning gives another handle to the same switch.
#[derive(Debug, Clone)]
pub(crate) struct Shutdown {
    started: watch::Sender<bool>,
}

impl Shutdown {
    pub(crate) fn new() -> Self {
        Self {
            started: watch::Sender::new(false),
        }
    }

    /// Starts the shutdown. Doing it twice changes nothing.
    pub(crate) fn start(&self) {
        self.started.send_replace(true);
    }

    pub(crate) fn has_started(&self) -> bool {
        *self.started.borrow()
    }

    /// Completes once the shutdown has started, immediately if it already has.
    pub(crate) async fn wait(&self) {
        let mut started = self.started.subscribe();
        // The sender lives in `self`, so it cannot be gone while this future runs; `Err` is unreachable.
        let _ = started.wait_for(|started| *started).await;
    }
}

/// Serves `router` until `shutdown` starts. From then on no new connection is accepted, and requests in flight
/// get up to `drain` to finish.
///
/// It returns when the last connection has closed or when `drain` is over, whichever comes first. A request
/// still running then is not waited for any longer; its connection ends when the runtime stops, which the
/// binary does right after this returns.
pub(crate) async fn serve(
    listener: TcpListener,
    router: Router,
    shutdown: Shutdown,
    drain: Duration,
) -> io::Result<()> {
    let signal = shutdown.clone();
    let server = axum::serve(listener, router).with_graceful_shutdown(async move { signal.wait().await });
    let deadline = async {
        shutdown.wait().await;
        tokio::time::sleep(drain).await;
    };
    tokio::select! {
        result = server => result,
        () = deadline => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::time::Instant;

    use axum::routing::get;
    use tokio::sync::oneshot;

    use super::*;

    /// A server with one route that answers only after `release` fires (or never, if it is dropped unfired).
    async fn slow_server(drain: Duration) -> (SocketAddr, Shutdown, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
        let (release, released) = oneshot::channel::<()>();
        let released = std::sync::Arc::new(tokio::sync::Mutex::new(Some(released)));
        let router = Router::new().route(
            "/slow",
            get(move || {
                let released = released.clone();
                async move {
                    let receiver = released.lock().await.take();
                    let released = match receiver {
                        Some(receiver) => receiver.await.is_ok(),
                        None => false,
                    };
                    if released { "done" } else { std::future::pending().await }
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Shutdown::new();
        let task = tokio::spawn({
            let shutdown = shutdown.clone();
            async move { serve(listener, router, shutdown, drain).await.unwrap() }
        });
        (address, shutdown, release, task)
    }

    #[tokio::test]
    async fn an_in_flight_request_completes_and_new_connections_are_refused() {
        let (address, shutdown, release, server) = slow_server(Duration::from_secs(30)).await;
        let client = reqwest::Client::new();
        let in_flight = tokio::spawn(client.get(format!("http://{address}/slow")).send());
        // Give the request time to reach the handler before shutting down.
        tokio::time::sleep(Duration::from_millis(200)).await;

        shutdown.start();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!server.is_finished(), "the server must wait for the request in flight");
        let refused = reqwest::Client::new()
            .get(format!("http://{address}/slow"))
            .send()
            .await;
        assert!(
            refused.is_err(),
            "a new connection must not be accepted during shutdown"
        );

        release.send(()).unwrap();
        let response = in_flight.await.unwrap().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.unwrap(), "done");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_request_that_outlasts_the_drain_time_is_not_waited_for() {
        let (address, shutdown, _never_released, server) = slow_server(Duration::from_millis(300)).await;
        let in_flight = tokio::spawn(reqwest::Client::new().get(format!("http://{address}/slow")).send());
        tokio::time::sleep(Duration::from_millis(200)).await;

        let started = Instant::now();
        shutdown.start();
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("serve returns once the drain time is over")
            .unwrap();

        assert!(started.elapsed() >= Duration::from_millis(300));
        assert!(!in_flight.is_finished(), "the request never got an answer");
        in_flight.abort();
    }

    #[tokio::test]
    async fn without_requests_shutdown_is_immediate() {
        let (_address, shutdown, _release, server) = slow_server(Duration::from_secs(30)).await;

        let started = Instant::now();
        shutdown.start();
        shutdown.start();
        server.await.unwrap();

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(shutdown.has_started());
    }
}
