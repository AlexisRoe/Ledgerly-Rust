//! Discovery and the JWK set against a provider served in-process on 127.0.0.1.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;

use super::*;

/// What the fake provider answers, and how often it was asked.
#[derive(Clone, Default)]
struct Provider {
    /// The issuer its configuration names; `None`: the one it is reached at.
    issuer: Option<String>,
    /// Answer the OpenID location with this status instead of the document.
    oidc_status: Option<StatusCode>,
    /// Fail this many configuration requests with a 500 before answering.
    failures: Arc<AtomicUsize>,
    configuration_requests: Arc<AtomicUsize>,
    key_requests: Arc<AtomicUsize>,
}

const KEYS: &str = r#"{"keys": [
    {"kid": "k1", "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig",
     "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "y": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"},
    {"kid": "e1", "kty": "RSA", "alg": "RSA-OAEP", "use": "enc", "n": "AQAB", "e": "AQAB"}
]}"#;

/// Starts the provider; returns its issuer, `http://127.0.0.1:<port>/realms/ledgerly`.
async fn start(provider: Provider) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let issuer = format!("{origin}/realms/ledgerly");
    let provider = Provider {
        issuer: Some(provider.issuer.clone().unwrap_or_else(|| issuer.clone())),
        ..provider
    };
    let router = Router::new()
        .route("/realms/ledgerly/.well-known/openid-configuration", get(oidc))
        .route("/.well-known/openid-configuration/realms/ledgerly", get(configuration))
        .route("/realms/ledgerly/certs", get(keys))
        .with_state((provider, origin));
    tokio::spawn(async move { axum::serve(listener, router).await });
    issuer
}

async fn oidc(State((provider, origin)): State<(Provider, String)>) -> Response {
    match provider.oidc_status {
        Some(status) => status.into_response(),
        None => configuration(State((provider, origin))).await,
    }
}

async fn configuration(State((provider, origin)): State<(Provider, String)>) -> Response {
    provider.configuration_requests.fetch_add(1, Ordering::SeqCst);
    if provider
        .failures
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1))
        .is_ok()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    axum::Json(json!({
        "issuer": provider.issuer,
        "jwks_uri": format!("{origin}/realms/ledgerly/certs"),
    }))
    .into_response()
}

async fn keys(State((provider, _)): State<(Provider, String)>) -> Response {
    provider.key_requests.fetch_add(1, Ordering::SeqCst);
    ([(axum::http::header::CONTENT_TYPE, "application/json")], KEYS).into_response()
}

#[tokio::test]
async fn discovery_finds_the_keys_and_the_algorithms_they_allow() {
    let issuer = start(Provider::default()).await;

    let keys = Issuer::new(&issuer).unwrap().keys().await.unwrap();

    assert_eq!(keys.keys.len(), 1, "the encryption key is not used");
    assert_eq!(&*keys.algorithms, [Algorithm::Es256]);
}

#[tokio::test]
async fn nothing_is_asked_before_the_first_token_and_the_keys_are_kept() {
    let provider = Provider::default();
    let issuer = Issuer::new(&start(provider.clone()).await).unwrap();
    assert_eq!(provider.configuration_requests.load(Ordering::SeqCst), 0);

    issuer.keys().await.unwrap();
    issuer.keys().await.unwrap();

    assert_eq!(provider.configuration_requests.load(Ordering::SeqCst), 1);
    assert_eq!(provider.key_requests.load(Ordering::SeqCst), 1);
}

/// Spring tries the RFC 8414 location after a 4xx on the OpenID one.
#[tokio::test]
async fn after_a_client_error_the_next_location_is_tried() {
    let issuer = start(Provider {
        oidc_status: Some(StatusCode::NOT_FOUND),
        ..Provider::default()
    })
    .await;

    assert!(Issuer::new(&issuer).unwrap().keys().await.is_ok());
}

#[tokio::test]
async fn a_server_error_ends_the_discovery_and_the_next_token_tries_again() {
    let provider = Provider {
        failures: Arc::new(AtomicUsize::new(1)),
        ..Provider::default()
    };
    let issuer = Issuer::new(&start(provider.clone()).await).unwrap();

    assert_eq!(
        issuer.keys().await.unwrap_err(),
        Unreachable("the provider answered with an error")
    );
    assert!(issuer.keys().await.is_ok());
    assert_eq!(provider.configuration_requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_configuration_naming_another_issuer_is_not_used() {
    let issuer = start(Provider {
        issuer: Some("http://elsewhere.test/realms/ledgerly".to_owned()),
        ..Provider::default()
    })
    .await;

    assert_eq!(
        Issuer::new(&issuer).unwrap().keys().await.unwrap_err(),
        Unreachable("the provider's configuration names another issuer")
    );
}

#[tokio::test]
async fn a_provider_that_is_not_there_is_unreachable() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    let issuer = Issuer::new(&format!("http://{address}/realms/ledgerly")).unwrap();

    assert_eq!(issuer.keys().await.unwrap_err(), Unreachable("no connection"));
    assert_eq!(issuer.refetch().await.map(|keys| keys.keys.len()), None);
}

/// Nimbus asks again for an unknown key at most every 30 seconds.
#[tokio::test]
async fn the_keys_are_fetched_again_for_an_unknown_key_at_most_every_thirty_seconds() {
    let provider = Provider::default();
    let issuer = Issuer::new(&start(provider.clone()).await).unwrap();
    issuer.keys().await.unwrap();

    assert!(issuer.refetch().await.is_some());
    assert!(issuer.refetch().await.is_none());
    assert_eq!(provider.key_requests.load(Ordering::SeqCst), 2);
}

#[test]
fn the_discovery_locations_are_springs() {
    let issuer: Uri = "https://id.example.com/realms/ledgerly".parse().unwrap();

    let locations = configuration_locations(&issuer).unwrap().map(|uri| uri.to_string());

    assert_eq!(
        locations,
        [
            "https://id.example.com/realms/ledgerly/.well-known/openid-configuration",
            "https://id.example.com/.well-known/openid-configuration/realms/ledgerly",
            "https://id.example.com/.well-known/oauth-authorization-server/realms/ledgerly",
        ]
    );
}
