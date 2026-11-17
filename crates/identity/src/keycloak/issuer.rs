//! The identity provider over HTTP: OpenID discovery, then its JWK set. The only code of this crate that calls
//! out, and only to the configured issuer and the `jwks_uri` it announces.
//!
//! Java counterpart: `JwtDecoders.fromIssuerLocation` inside the `SupplierJwtDecoder` of
//! `KeycloakResourceServerConfig`, and the remote JWK source Spring's `NimbusJwtDecoder` builds on `jwks_uri`.
//!
//! - Nothing is fetched at start-up, so Ledgerly starts while Keycloak is still starting. The first token
//!   triggers discovery, and until it succeeds every token triggers it again (Spring's `SupplierJwtDecoder`
//!   remembers only a decoder it could build).
//! - Discovery tries, in Spring's order, `{issuer}/.well-known/openid-configuration`, then the RFC 8414 forms
//!   `{origin}/.well-known/openid-configuration{path}` and `{origin}/.well-known/oauth-authorization-server
//!   {path}`, going on to the next only after a 4xx. The document must name a `jwks_uri` and the configured
//!   issuer exactly.
//! - The JWK set is fetched with the discovery, and decides once which algorithms are accepted. It is fetched
//!   again when it is more than five minutes old, and when a token names a key it does not have, at most
//!   every 30 seconds (Nimbus' cache and rate limit). If fetching it again fails, the keys already known stay
//!   in use.
//!
//! Unlike Java's `RestTemplate`, requests time out (5 s to connect, 10 s in all), a response is read up to
//! 1 MiB, and redirects are not followed, so that a hanging, babbling or wandering provider cannot hold
//! requests forever. HTTP/1.1 over rustls (with `ring`), trusting the system's CA certificates.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::{Request, StatusCode, Uri, header};
use http_body_util::{BodyExt, Empty, Limited};
use hyper::body::Bytes;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde_json::Value;
use tokio::sync::Mutex;

use super::jwk::{self, Algorithm, Jwk};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_DOCUMENT: usize = 1 << 20;
/// Nimbus' default cache time to live for a remote JWK set.
const KEYS_TIME_TO_LIVE: Duration = Duration::from_secs(5 * 60);
/// Nimbus' default minimum interval between two fetches for an unknown key.
const REFETCH_INTERVAL: Duration = Duration::from_secs(30);

/// The provider could not be asked, or its answer was unusable. The reason names no URL and no token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub(crate) struct Unreachable(pub &'static str);

/// TLS for the HTTP client could not be set up. The application does not start.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct TlsSetupFailed(pub String);

/// What a token is checked against.
#[derive(Debug, Clone)]
pub(crate) struct Keys {
    pub algorithms: Arc<[Algorithm]>,
    pub keys: Arc<[Jwk]>,
}

#[derive(Debug)]
struct Discovered {
    jwks_uri: Uri,
    algorithms: Arc<[Algorithm]>,
    keys: Arc<[Jwk]>,
    fetched_at: Instant,
    last_refetch: Option<Instant>,
}

impl Discovered {
    fn keys(&self) -> Keys {
        Keys {
            algorithms: self.algorithms.clone(),
            keys: self.keys.clone(),
        }
    }
}

/// The configured identity provider.
#[derive(Debug)]
pub(crate) struct Issuer {
    issuer: String,
    client: Client<HttpsConnector<HttpConnector>, Empty<Bytes>>,
    state: Mutex<Option<Discovered>>,
}

impl Issuer {
    /// An issuer that is asked nothing yet. The HTTP client trusts the system's CA certificates (the image has
    /// Debian's), as Java trusts the JVM's.
    pub(crate) fn new(issuer: &str) -> Result<Self, TlsSetupFailed> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut http = HttpConnector::new();
        http.set_connect_timeout(Some(CONNECT_TIMEOUT));
        http.enforce_http(false);
        let https = HttpsConnectorBuilder::new()
            .with_provider_and_native_roots(provider)
            .map_err(|error| TlsSetupFailed(error.to_string()))?
            .https_or_http()
            .enable_http1()
            .wrap_connector(http);
        Ok(Self {
            issuer: issuer.to_owned(),
            client: Client::builder(TokioExecutor::new()).build(https),
            state: Mutex::new(None),
        })
    }

    /// The configured issuer, which a token's `iss` must equal.
    pub(crate) fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The keys, after discovery on first use, fetched again when they are old.
    pub(crate) async fn keys(&self) -> Result<Keys, Unreachable> {
        let mut state = self.state.lock().await;
        if let Some(discovered) = state.as_mut() {
            if discovered.fetched_at.elapsed() > KEYS_TIME_TO_LIVE
                && let Ok(keys) = self.fetch_keys(&discovered.jwks_uri).await
            {
                discovered.keys = keys.into();
                discovered.fetched_at = Instant::now();
            }
            return Ok(discovered.keys());
        }
        let discovered = self.discover().await?;
        let keys = discovered.keys();
        *state = Some(discovered);
        Ok(keys)
    }

    /// The keys fetched again because a token names one that is not among them; `None` if they were fetched
    /// less than 30 seconds ago or cannot be fetched now.
    pub(crate) async fn refetch(&self) -> Option<Keys> {
        let mut state = self.state.lock().await;
        let discovered = state.as_mut()?;
        if discovered
            .last_refetch
            .is_some_and(|last| last.elapsed() < REFETCH_INTERVAL)
        {
            return None;
        }
        discovered.last_refetch = Some(Instant::now());
        let keys = self.fetch_keys(&discovered.jwks_uri).await.ok()?;
        discovered.keys = keys.into();
        discovered.fetched_at = Instant::now();
        Some(discovered.keys())
    }

    async fn discover(&self) -> Result<Discovered, Unreachable> {
        let configuration = self.configuration().await?;
        let jwks_uri = configuration
            .get("jwks_uri")
            .and_then(Value::as_str)
            .and_then(|uri| uri.parse::<Uri>().ok())
            .ok_or(Unreachable("the provider's configuration has no jwks_uri"))?;
        if configuration.get("issuer").and_then(Value::as_str) != Some(self.issuer.as_str()) {
            return Err(Unreachable("the provider's configuration names another issuer"));
        }
        let keys = self.fetch_keys(&jwks_uri).await?;
        let algorithms = jwk::algorithms(&keys);
        if algorithms.is_empty() {
            return Err(Unreachable("the provider's JWK set has no signing key"));
        }
        Ok(Discovered {
            jwks_uri,
            algorithms: algorithms.into(),
            keys: keys.into(),
            fetched_at: Instant::now(),
            last_refetch: None,
        })
    }

    /// Java: `JwtDecoderProviderConfigurationUtils.getConfigurationForIssuerLocation`.
    async fn configuration(&self) -> Result<serde_json::Map<String, Value>, Unreachable> {
        let issuer: Uri = self
            .issuer
            .parse()
            .map_err(|_| Unreachable("the issuer is not a URL"))?;
        for location in configuration_locations(&issuer)? {
            match self.get_json(location).await {
                Ok(Value::Object(configuration)) => return Ok(configuration),
                Ok(_) => return Err(Unreachable("the provider's configuration is not a JSON object")),
                Err(Fetch::ClientError) => {}
                Err(Fetch::Failed(reason)) => return Err(reason),
            }
        }
        Err(Unreachable("the provider has no configuration document"))
    }

    async fn fetch_keys(&self, jwks_uri: &Uri) -> Result<Vec<Jwk>, Unreachable> {
        let document = self.get_json(jwks_uri.clone()).await.map_err(|failure| match failure {
            Fetch::ClientError => Unreachable("the provider refused the JWK set request"),
            Fetch::Failed(reason) => reason,
        })?;
        jwk::parse_set(&document).ok_or(Unreachable("the provider's JWK set is not one"))
    }

    async fn get_json(&self, uri: Uri) -> Result<Value, Fetch> {
        let request = Request::get(uri)
            .header(header::ACCEPT, "application/json")
            .body(Empty::new())
            .map_err(|_| Fetch::Failed(Unreachable("the request could not be built")))?;
        let exchange = async {
            let response = self
                .client
                .request(request)
                .await
                .map_err(|_| Fetch::Failed(Unreachable("no connection")))?;
            let status = response.status();
            if status.is_client_error() {
                return Err(Fetch::ClientError);
            }
            if status != StatusCode::OK {
                return Err(Fetch::Failed(Unreachable("the provider answered with an error")));
            }
            Limited::new(response.into_body(), MAX_DOCUMENT)
                .collect()
                .await
                .map(|body| body.to_bytes())
                .map_err(|_| Fetch::Failed(Unreachable("the answer broke off or is too large")))
        };
        let body = tokio::time::timeout(TIMEOUT, exchange)
            .await
            .map_err(|_| Fetch::Failed(Unreachable("no answer in time")))??;
        serde_json::from_slice(&body).map_err(|_| Fetch::Failed(Unreachable("the answer is not JSON")))
    }
}

enum Fetch {
    /// A 4xx: discovery goes on with the next location.
    ClientError,
    Failed(Unreachable),
}

/// Spring's three discovery locations for an issuer, in order.
fn configuration_locations(issuer: &Uri) -> Result<[Uri; 3], Unreachable> {
    let (Some(scheme), Some(authority)) = (issuer.scheme_str(), issuer.authority()) else {
        return Err(Unreachable("the issuer is not a URL"));
    };
    let origin = format!("{scheme}://{authority}");
    let path = issuer.path().trim_end_matches('/');
    let parse = |location: String| {
        location
            .parse::<Uri>()
            .map_err(|_| Unreachable("the issuer is not a URL"))
    };
    Ok([
        parse(format!("{origin}{path}/.well-known/openid-configuration"))?,
        parse(format!("{origin}/.well-known/openid-configuration{path}"))?,
        parse(format!("{origin}/.well-known/oauth-authorization-server{path}"))?,
    ])
}

#[cfg(test)]
mod tests;
